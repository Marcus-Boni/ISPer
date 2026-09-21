//! Gravador de reuniões (Fase 4): microfone + áudio do sistema (loopback
//! WASAPI), transcritos em blocos DURANTE a gravação.
//!
//! Sem bot e sem API paga: o Windows deixa capturar o que sai da caixa de
//! som (loopback — módulo [`crate::loopback`]). Quem veio do mic é "Eu";
//! quem veio do loopback é "Participantes". Cada canal é fatiado em blocos
//! de ~20 s — cortados num ponto de silêncio para não partir palavra — e
//! transcrito em paralelo à gravação: ao encerrar, só o resto é processado.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};

use crate::align::SpeakerTurn;
use crate::audio::RawAudio;
use crate::chunk::{self, ChunkOptions, CutReason};
use crate::loopback::LoopbackSource;
use crate::{IsperError, Result, WhisperEngine, audio, loopback};

/// Recebe cada fala assim que o bloco dela é transcrito — é a transcrição
/// "ao vivo": chega com o atraso de um bloco (~20 s) + a inferência.
pub type SegmentSink = Arc<dyn Fn(&MeetingSegment) + Send + Sync>;

/// Um bloco de áudio que passou pelo Whisper (ou falhou), para métricas locais.
#[derive(Debug, Clone, Copy)]
pub struct BlockStats {
    pub speaker: Speaker,
    /// Duração do áudio do bloco, em segundos.
    pub block_secs: f32,
    /// Quanto a inferência levou; `None` = o bloco falhou.
    pub infer_secs: Option<f32>,
}

/// Chamado a cada bloco transcrito ou que falhou ([`BlockStats`]).
pub type BlockSink = Arc<dyn Fn(&BlockStats) + Send + Sync>;

/// Opções de uma gravação de reunião.
#[derive(Clone)]
pub struct MeetingOptions {
    pub lang: String,
    pub initial_prompt: Option<String>,
    /// De onde vem o áudio dos "Participantes" (sistema inteiro ou só um app).
    pub source: LoopbackSource,
    /// Microfone do canal "Eu" (`None` = padrão do sistema).
    pub input_device: Option<String>,
    /// Chamado a cada segmento transcrito durante a gravação.
    pub on_segment: Option<SegmentSink>,
    /// Chamado com a legenda PROVISÓRIA do bloco ainda aberto (a cada
    /// [`PARTIAL_EVERY`], se o worker estiver livre). O bloco final chega em
    /// seguida por `on_segment` e a substitui; ela não entra na ata.
    pub on_partial: Option<SegmentSink>,
    /// Chamado a cada bloco transcrito (ou que falhou): duração do áudio e
    /// tempo de inferência, para as métricas locais do app.
    pub on_block: Option<BlockSink>,
    /// Dicionário pessoal: além de virar `initial_prompt`, corrige por
    /// semelhança o que o Whisper ainda errar ([`crate::text::apply_dictionary`]).
    pub dictionary: Vec<String>,
}

/// Regras de agrupamento de falas em parágrafos (Markdown, DOCX e Biblioteca
/// usam as mesmas): o mesmo falante continua no parágrafo só enquanto a pausa
/// entre falas for menor que `GROUP_GAP_SECS` e o parágrafo não passar de
/// `GROUP_MAX_SECS` — sem isso, uma hora de "Participantes" vira uma parede
/// de texto sem horário.
pub const GROUP_MAX_SECS: f32 = 60.0;
pub const GROUP_GAP_SECS: f32 = 4.0;

/// Visão emprestada de um segmento — serve tanto ao resultado recém-gravado
/// quanto ao que já está no banco.
#[derive(Debug, Clone, Copy)]
pub struct SegmentRef<'a> {
    pub speaker: &'a str,
    pub start_secs: f32,
    pub end_secs: f32,
    pub text: &'a str,
}

/// Um parágrafo: falas consecutivas do mesmo falante, dentro das regras acima.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeechGroup {
    pub speaker: String,
    pub start_secs: f32,
    pub end_secs: f32,
    pub text: String,
}

pub fn group_speech<'a>(segments: impl IntoIterator<Item = SegmentRef<'a>>) -> Vec<SpeechGroup> {
    let mut groups: Vec<SpeechGroup> = Vec::new();
    for s in segments {
        let text = s.text.trim();
        if text.is_empty() {
            continue;
        }
        let continues = groups.last().is_some_and(|g| {
            g.speaker == s.speaker
                && s.start_secs - g.end_secs < GROUP_GAP_SECS
                && s.end_secs - g.start_secs <= GROUP_MAX_SECS
        });
        match groups.last_mut() {
            Some(g) if continues => {
                g.text.push(' ');
                g.text.push_str(text);
                g.end_secs = g.end_secs.max(s.end_secs);
            }
            _ => groups.push(SpeechGroup {
                speaker: s.speaker.to_string(),
                start_secs: s.start_secs,
                end_secs: s.end_secs,
                text: text.to_string(),
            }),
        }
    }
    groups
}

/// Blocos mais silenciosos que isso nem vão para o Whisper (economiza GPU e
/// evita as alucinações clássicas em silêncio, tipo "Legendas pela...").
/// O áudio silencioso continua indo para o arquivo do passe final — é o que
/// mantém a linha do tempo contínua.
const SILENCE_RMS: f32 = 0.0035;
/// Com que frequência vale reavaliar se já dá para cortar o buffer. O fatiador
/// recebe pacotes de ~10 ms; procurar silêncio a cada um deles seria gastar
/// CPU à toa.
const CUT_CHECK_EVERY: Duration = Duration::from_millis(400);
/// Legenda provisória: de quanto em quanto tempo o buffer aberto vai ao
/// Whisper enquanto o bloco não fecha (só com o worker livre).
pub const PARTIAL_EVERY: Duration = Duration::from_millis(2500);
/// Menos áudio que isto não rende legenda provisória.
const PARTIAL_MIN_SECS: f32 = 1.5;
/// Com esta quantidade de blocos esperando, o worker está atrasado: a captura
/// volta aos blocos longos ([`ChunkOptions::relaxed`]) até a fila esvaziar.
const BACKLOG_RELAX: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
    Me,
    /// Alguém do loopback, sem identificação individual.
    Others,
    /// Alguém do loopback identificado pela diarização (1, 2, 3…).
    ///
    /// `u32` e não `u8`: saturar em 255 transformava um agrupamento quebrado
    /// no rótulo "Participante 255" em vez de num erro visível.
    Participant(u32),
}

impl Speaker {
    pub fn label(&self) -> String {
        match self {
            Speaker::Me => "Eu".into(),
            Speaker::Others => "Participantes".into(),
            Speaker::Participant(n) => format!("Participante {n}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MeetingSegment {
    pub speaker: Speaker,
    pub start_secs: f32,
    pub end_secs: f32,
    pub text: String,
}

pub struct MeetingResult {
    /// Segmentos dos dois canais, em ordem cronológica (transcrição AO VIVO).
    pub segments: Vec<MeetingSegment>,
    pub duration_secs: f32,
    /// Áudio dos participantes (16 kHz mono), no RELÓGIO DA REUNIÃO — os
    /// trechos sem entrega viram silêncio, de modo que a posição no arquivo é
    /// o instante da reunião. Insumo da diarização e do passe final.
    pub others_audio: ChannelAudio,
    /// Idem para o microfone ("Eu"). Só o passe final usa.
    pub me_audio: ChannelAudio,
    /// Quantos blocos precisaram ser cortados sem silêncio à vista (o único
    /// caso em que o ao vivo pode partir uma palavra).
    pub forced_cuts: usize,
}

/// Áudio de um canal (16 kHz mono, PCM 16 bits little-endian) gravado
/// DURANTE a reunião num arquivo temporário — não em RAM. Uma hora são 115 MB
/// em i16; duas horas em memória empurravam o app para centenas de MB à toa,
/// sendo que o áudio só é lido de volta depois que a reunião termina. O
/// arquivo é apagado quando este valor é descartado.
///
/// A posição no arquivo é o INSTANTE DA REUNIÃO: o que não chegou (pausa na
/// reprodução, dispositivo reaberto) entra como silêncio. Sem isso, o áudio
/// concatenado emenda trechos distantes, e o segmentador da diarização vê uma
/// troca de voz onde só houve uma emenda — era daí que saíam dezenas de
/// "participantes" numa reunião de seis pessoas.
pub struct ChannelAudio {
    path: Option<PathBuf>,
    samples: u64,
}

impl ChannelAudio {
    /// Sem áudio (reunião sem participantes, ou sem arquivo temporário).
    pub fn empty() -> Self {
        Self {
            path: None,
            samples: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.samples == 0
    }

    /// Amostras gravadas (a 16 kHz).
    pub fn samples(&self) -> u64 {
        self.samples
    }

    /// Duração do áudio guardado, em segundos.
    pub fn secs(&self) -> f32 {
        self.samples as f32 / crate::WHISPER_SAMPLE_RATE as f32
    }

    /// Lê tudo de volta como f32 normalizado — o formato que a diarização
    /// espera. É o único momento em que o áudio inteiro fica em memória.
    pub fn read_f32(&self) -> Result<Vec<f32>> {
        let Some(path) = &self.path else {
            return Ok(Vec::new());
        };
        let mut reader = BufReader::with_capacity(1 << 16, File::open(path)?);
        let mut out: Vec<f32> = Vec::with_capacity(self.samples as usize);
        let mut buf = vec![0u8; 1 << 16];
        // Uma leitura pode terminar no meio de uma amostra (2 bytes): o byte
        // que sobra espera o próximo bloco em `pending`.
        let mut pending: Vec<u8> = Vec::with_capacity(1 << 16);
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            pending.extend_from_slice(&buf[..n]);
            let even = pending.len() - pending.len() % 2;
            out.extend(
                pending[..even]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| i16::from_le_bytes(*b) as f32 / i16::MAX as f32),
            );
            pending.drain(..even);
        }
        Ok(out)
    }
}

impl Drop for ChannelAudio {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Escritor do [`OthersAudio`]: converte cada bloco f32 para i16 e o anexa ao
/// arquivo temporário, com buffer — a thread de transcrição não espera o disco.
struct PcmSpool {
    file: BufWriter<File>,
    path: PathBuf,
    samples: u64,
}

/// Distingue arquivos de reuniões abertas ao mesmo tempo no mesmo processo.
static SPOOL_SEQ: AtomicU64 = AtomicU64::new(0);

/// Restos de sessões que caíram ficam em `%TEMP%\ISPer`; mais velhos que isso, somem.
const SPOOL_STALE: Duration = Duration::from_secs(24 * 3600);

impl PcmSpool {
    fn create() -> Result<Self> {
        let dir = std::env::temp_dir().join("ISPer");
        std::fs::create_dir_all(&dir)?;
        Self::sweep_stale(&dir);
        let seq = SPOOL_SEQ.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("participantes-{}-{seq}.pcm", std::process::id()));
        let file = File::options().write(true).create_new(true).open(&path)?;
        Ok(Self {
            file: BufWriter::with_capacity(1 << 16, file),
            path,
            samples: 0,
        })
    }

    /// Apaga `.pcm` antigos deixados por um processo que não chegou ao `Drop`.
    fn sweep_stale(dir: &std::path::Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let now = std::time::SystemTime::now();
        for entry in entries.flatten() {
            let path = entry.path();
            let is_pcm = path.extension().is_some_and(|e| e == "pcm");
            let old = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| now.duration_since(t).ok())
                .is_some_and(|age| age > SPOOL_STALE);
            if is_pcm && old {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    /// Grava `samples` na posição correspondente a `wall_start` segundos de
    /// reunião, preenchendo com silêncio o que faltar. É o que faz a posição
    /// no arquivo ser o instante da reunião.
    fn push_at(&mut self, wall_start: f32, samples: &[f32]) -> Result<()> {
        let alvo = (wall_start.max(0.0) * crate::WHISPER_SAMPLE_RATE as f32) as u64;
        if alvo > self.samples {
            self.pad(alvo - self.samples)?;
        }
        let mut bytes: Vec<u8> = Vec::with_capacity(samples.len() * 2);
        for s in samples {
            let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        self.file.write_all(&bytes)?;
        self.samples += samples.len() as u64;
        Ok(())
    }

    /// Escreve `n` amostras de silêncio, em pedaços — um buraco de minutos
    /// não pode virar um `Vec` de dezenas de MB.
    fn pad(&mut self, n: u64) -> Result<()> {
        const PEDACO: usize = 1 << 15;
        let zeros = [0u8; PEDACO * 2];
        let mut faltam = n;
        while faltam > 0 {
            let agora = faltam.min(PEDACO as u64) as usize;
            self.file.write_all(&zeros[..agora * 2])?;
            faltam -= agora as u64;
        }
        self.samples += n;
        Ok(())
    }

    /// Completa o arquivo com silêncio até `secs` de reunião, para que a
    /// duração do canal seja a da reunião mesmo que o áudio tenha acabado
    /// antes.
    fn pad_to(&mut self, secs: f32) -> Result<()> {
        let alvo = (secs.max(0.0) * crate::WHISPER_SAMPLE_RATE as f32) as u64;
        if alvo > self.samples {
            self.pad(alvo - self.samples)?;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<ChannelAudio> {
        self.file.flush()?;
        Ok(ChannelAudio {
            path: Some(std::mem::take(&mut self.path)),
            samples: self.samples,
        })
    }
}

impl Drop for PcmSpool {
    /// Um spool descartado sem `finish` (erro de escrita) não deixa lixo.
    fn drop(&mut self) {
        if !self.path.as_os_str().is_empty() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

impl MeetingResult {
    /// Áudio dos participantes em f32 normalizado, como a diarização espera
    /// (lido do arquivo temporário — ver [`ChannelAudio::read_f32`]).
    pub fn others_audio_f32(&self) -> Result<Vec<f32>> {
        self.others_audio.read_f32()
    }

    /// Áudio do microfone em f32 normalizado (insumo do passe final).
    pub fn me_audio_f32(&self) -> Result<Vec<f32>> {
        self.me_audio.read_f32()
    }

    /// Aplica turnos de falante aos segmentos "Participantes": cada segmento
    /// vira "Participante N" do turno com maior sobreposição.
    ///
    /// Os turnos vêm no relógio da reunião — o mesmo do áudio gravado, porque
    /// [`ChannelAudio`] preenche as lacunas com silêncio. Antes havia um mapa
    /// bloco a bloco entre o "áudio concatenado" e o relógio; ele sumiu junto
    /// com a concatenação.
    ///
    /// Atribuição por SEGMENTO é a granularidade do ao vivo. O passe final
    /// ([`crate::pipeline`]) atribui por palavra.
    pub fn apply_speaker_turns(&mut self, turns: &[SpeakerTurn]) {
        for seg in self.segments.iter_mut() {
            if seg.speaker != Speaker::Others {
                continue;
            }
            let cs = seg.start_secs;
            let ce = seg.end_secs.max(cs + 0.1);
            let best = turns
                .iter()
                .map(|t| {
                    (
                        (t.end_secs.min(ce) - t.start_secs.max(cs)).max(0.0),
                        t.speaker,
                    )
                })
                .filter(|(overlap, _)| *overlap > 0.0)
                .max_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, spk)) = best {
                seg.speaker = Speaker::Participant(spk + 1);
            }
        }
    }

    /// Quantos participantes distintos foram identificados.
    pub fn distinct_participants(&self) -> usize {
        let mut ids: Vec<u32> = self
            .segments
            .iter()
            .filter_map(|s| match s.speaker {
                Speaker::Participant(n) => Some(n),
                _ => None,
            })
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids.len()
    }
}

pub struct MeetingHandle {
    stop_txs: Vec<Sender<()>>,
    done_rx: Receiver<Result<MeetingResult>>,
    started: Instant,
    /// Avisos não-fatais da abertura (ex.: Teams não encontrado → sistema).
    pub warnings: Vec<String>,
}

impl MeetingHandle {
    /// Encerra a gravação e espera a transcrição dos blocos restantes.
    pub fn stop(self) -> Result<MeetingResult> {
        for tx in &self.stop_txs {
            let _ = tx.send(());
        }
        self.done_rx
            .recv()
            .map_err(|_| IsperError::Audio("worker da reunião encerrou inesperadamente".into()))?
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}

/// Um bloco pronto para o worker: de qual canal veio, em que instante da
/// reunião começa, o áudio e se o corte foi forçado (sem silêncio à vista).
struct Job {
    speaker: Speaker,
    offset: f32,
    audio: RawAudio,
    forced_cut: bool,
    /// Legenda provisória (buffer ainda aberto): vai à tela, não à ata nem
    /// ao arquivo do passe final.
    provisional: bool,
}

/// Inicia a gravação nos dois canais. Valida que ambos abriram antes de
/// retornar — se o loopback ou o mic falhar, você fica sabendo já.
pub fn start(engine: Arc<WhisperEngine>, opts: MeetingOptions) -> Result<MeetingHandle> {
    let (job_tx, job_rx) = unbounded::<Job>();
    let (done_tx, done_rx) = unbounded();
    let (ready_tx, ready_rx) = unbounded::<Result<Option<String>>>();
    let started = Instant::now();
    let MeetingOptions {
        lang,
        initial_prompt,
        source,
        input_device,
        on_segment,
        on_partial,
        on_block,
        dictionary,
    } = opts;

    // Abre os canais EM SEQUÊNCIA (loopback primeiro, mic depois): quando mic e
    // alto-falante são o mesmo dispositivo USB, abrir os dois ao mesmo tempo
    // fazia o loopback perder os primeiros segundos.
    let mut stop_txs = Vec::new();
    let mut warnings = Vec::new();
    for speaker in [Speaker::Others, Speaker::Me] {
        let (stop_tx, stop_rx) = unbounded::<()>();
        stop_txs.push(stop_tx);
        let job_tx = job_tx.clone();
        let ready_tx = ready_tx.clone();
        let source = source.clone();
        let device = input_device.clone();
        std::thread::spawn(move || {
            capture_channel(speaker, source, device, started, stop_rx, job_tx, ready_tx)
        });
        match ready_rx.recv_timeout(Duration::from_secs(8)) {
            Ok(Ok(None)) => {}
            Ok(Ok(Some(w))) => warnings.push(w),
            Ok(Err(e)) => {
                for tx in &stop_txs {
                    let _ = tx.send(());
                }
                return Err(e);
            }
            Err(_) => {
                for tx in &stop_txs {
                    let _ = tx.send(());
                }
                return Err(IsperError::Audio(
                    "timeout abrindo captura da reunião".into(),
                ));
            }
        }
    }
    // O worker termina quando os DOIS capturadores largarem o canal de jobs.
    drop(job_tx);

    let worker_opts = TranscribeOptions {
        lang,
        initial_prompt,
        dictionary,
        on_segment,
        on_partial,
        on_block,
    };
    std::thread::spawn(move || {
        let result = transcribe_worker(engine, worker_opts, job_rx, started);
        let _ = done_tx.send(result);
    });

    Ok(MeetingHandle {
        stop_txs,
        done_rx,
        started,
        warnings,
    })
}

/// Fonte de amostras de um canal da reunião, vista pelo fatiador de blocos.
/// Abstrai o microfone (cpal) e o loopback (wasapi) — e, nos testes, uma fonte
/// de mentira — para que o fatiador não conheça dispositivo nenhum.
trait AudioFeed {
    /// Canal por onde chegam as amostras f32 intercaladas.
    fn receiver(&self) -> &Receiver<Vec<f32>>;
    /// Taxa e canais do que chega pelo `receiver` (podem mudar num `reopen`:
    /// o fone sumiu e o padrão do sistema tem outro formato).
    fn format(&self) -> (u32, u16);
    /// Depois de quanto tempo sem NENHUMA amostra a fonte é dada como morta e
    /// reaberta. `None`: a fonte cuida disso sozinha (o loopback tem watchdog).
    fn stall_timeout(&self) -> Option<Duration>;
    /// Tolerância maior antes do primeiro pacote (fones Bluetooth demoram).
    fn first_packet_timeout(&self) -> Option<Duration> {
        self.stall_timeout().map(|d| d * 3)
    }
    /// Reabre a fonte depois de um stall (novo `receiver`/`format`).
    fn reopen(&mut self) -> Result<()>;
    /// Encerra a captura (chamado uma vez, ao parar).
    fn close(&mut self);
}

/// Microfone do canal "Eu" via cpal. Sabe se reabrir: se o fone for
/// desconectado no meio da reunião, [`audio::open_input_stream_on`] cai para
/// o microfone padrão do sistema e a gravação continua.
struct MicFeed {
    device: Option<String>,
    stream: Option<cpal::Stream>,
    rx: Receiver<Vec<f32>>,
    sample_rate: u32,
    channels: u16,
}

impl MicFeed {
    fn open(device: Option<String>) -> Result<Self> {
        use cpal::traits::StreamTrait;
        let (tx, rx) = unbounded();
        let (stream, sample_rate, channels) = audio::open_input_stream_on(device.as_deref(), tx)?;
        stream.play().map_err(audio::audio_err)?;
        Ok(Self {
            device,
            stream: Some(stream),
            rx,
            sample_rate,
            channels,
        })
    }
}

impl AudioFeed for MicFeed {
    fn receiver(&self) -> &Receiver<Vec<f32>> {
        &self.rx
    }
    fn format(&self) -> (u32, u16) {
        (self.sample_rate, self.channels)
    }
    fn stall_timeout(&self) -> Option<Duration> {
        Some(audio::MIC_STALL)
    }
    fn first_packet_timeout(&self) -> Option<Duration> {
        Some(audio::MIC_FIRST_PACKET)
    }
    fn reopen(&mut self) -> Result<()> {
        // O stream velho morre primeiro (mesma thread que o criou).
        drop(self.stream.take());
        *self = Self::open(self.device.clone())?;
        Ok(())
    }
    fn close(&mut self) {
        drop(self.stream.take()); // dropar o stream encerra a captura
    }
}

/// Loopback dos "Participantes": a thread do [`loopback::run`] entrega as
/// amostras e já reabre o cliente sozinha quando ele estagna.
struct LoopbackFeed {
    rx: Receiver<Vec<f32>>,
    sample_rate: u32,
    channels: u16,
    pump_stop_tx: Sender<()>,
}

impl AudioFeed for LoopbackFeed {
    fn receiver(&self) -> &Receiver<Vec<f32>> {
        &self.rx
    }
    fn format(&self) -> (u32, u16) {
        (self.sample_rate, self.channels)
    }
    fn stall_timeout(&self) -> Option<Duration> {
        None
    }
    fn reopen(&mut self) -> Result<()> {
        Ok(())
    }
    fn close(&mut self) {
        let _ = self.pump_stop_tx.send(());
    }
}

/// Abre a fonte certa para o canal e roda o fatiador de blocos.
fn capture_channel(
    speaker: Speaker,
    source: LoopbackSource,
    input_device: Option<String>,
    started: Instant,
    stop_rx: Receiver<()>,
    job_tx: Sender<Job>,
    ready_tx: Sender<Result<Option<String>>>,
) {
    match speaker {
        // "Eu": microfone via cpal, como no ditado.
        Speaker::Me => {
            let mut feed = match MicFeed::open(input_device) {
                Ok(feed) => feed,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let _ = ready_tx.send(Ok(None));
            chunk_loop(speaker, started, stop_rx, job_tx, &mut feed);
        }
        // "Participantes": loopback via wasapi, numa thread própria.
        // (Participant(_) só existe após a diarização — nunca chega aqui.)
        Speaker::Others | Speaker::Participant(_) => {
            let (pump_stop_tx, pump_stop_rx) = unbounded::<()>();
            let (data_tx, data_rx) = unbounded();
            let (lb_ready_tx, lb_ready_rx) = unbounded();
            std::thread::spawn(move || loopback::run(source, pump_stop_rx, data_tx, lb_ready_tx));
            let ready = match lb_ready_rx.recv_timeout(Duration::from_secs(8)) {
                Ok(Ok(ready)) => ready,
                Ok(Err(e)) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
                Err(_) => {
                    let _ = ready_tx.send(Err(IsperError::Audio("timeout no loopback".into())));
                    return;
                }
            };
            let mut feed = LoopbackFeed {
                rx: data_rx,
                sample_rate: ready.sample_rate,
                channels: ready.channels,
                pump_stop_tx,
            };
            let _ = ready_tx.send(Ok(ready.warning));
            chunk_loop(speaker, started, stop_rx, job_tx, &mut feed);
        }
    }
}

/// Entre avisos repetidos de "não consegui reabrir" (um fone que não voltou
/// não pode encher o log a cada dois segundos).
const REOPEN_WARN_EVERY: Duration = Duration::from_secs(30);

/// Acumula amostras de um canal e despacha blocos para transcrição. Se a
/// fonte parar de entregar (ver [`AudioFeed::stall_timeout`]), despacha o que
/// tem, pede para ela se reabrir e segue — a reunião não cai porque o fone
/// caiu.
fn chunk_loop(
    speaker: Speaker,
    started: Instant,
    stop_rx: Receiver<()>,
    job_tx: Sender<Job>,
    feed: &mut dyn AudioFeed,
) {
    let (mut sample_rate, mut channels) = feed.format();
    let live_opts = ChunkOptions::live();
    let relaxed_opts = ChunkOptions::relaxed();
    // Quando a última legenda provisória saiu (relógio de parede).
    let mut last_partial: Option<Instant> = None;
    let mut buf: Vec<f32> = Vec::new();
    // Quando foi a última vez que procuramos um ponto de corte.
    let mut last_cut_check: Option<Instant> = None;
    // Offset global (em segundos) do início do buffer atual. Marcado pelo
    // RELÓGIO da reunião quando o buffer começa a encher: o loopback só
    // entrega amostras enquanto algo está tocando, então contar amostras
    // subestimaria o tempo real durante as pausas.
    let mut buf_start_secs = 0.0f32;
    // Se a entrega parar por >0,5 s (pausa na reprodução), despachamos o
    // buffer e recarimbamos — cada trecho cai no timestamp certo mesmo com
    // entrega intermitente.
    let mut last_data_at: Option<Instant> = None;
    let mut last_reopen_warn: Option<Instant> = None;

    /// Despacha `buf` inteiro como um job (se tiver o mínimo de áudio).
    fn dispatch(
        job_tx: &Sender<Job>,
        speaker: Speaker,
        buf: &mut Vec<f32>,
        start_secs: f32,
        sample_rate: u32,
        channels: u16,
    ) {
        let min = (0.3 * sample_rate as f32) as usize * channels.max(1) as usize;
        if buf.len() >= min {
            let _ = job_tx.send(Job {
                speaker,
                offset: start_secs,
                audio: RawAudio {
                    samples: std::mem::take(buf),
                    sample_rate,
                    channels,
                },
                forced_cut: false,
                provisional: false,
            });
        } else {
            buf.clear();
        }
    }

    loop {
        let ch = channels.max(1) as usize;
        // Blocos curtos para a legenda chegar cedo; se o worker acumulou fila
        // (GPU ocupada, build CPU), blocos longos até ela esvaziar.
        let chunk_opts = if job_tx.len() >= BACKLOG_RELAX {
            &relaxed_opts
        } else {
            &live_opts
        };
        // Abaixo do alvo nem vale chamar o planejador de corte.
        let min_check_samples = (chunk_opts.target_secs * sample_rate as f32) as usize * ch;
        // Clonado a cada volta: um `reopen` troca o canal.
        let data_rx = feed.receiver().clone();
        let stall = if last_data_at.is_none() {
            feed.first_packet_timeout()
        } else {
            feed.stall_timeout()
        }
        .unwrap_or(Duration::from_secs(3600));

        crossbeam_channel::select! {
            recv(stop_rx) -> _ => {
                feed.close();
                std::thread::sleep(Duration::from_millis(60));
                for c in data_rx.try_iter() {
                    buf.extend(c);
                }
                dispatch(&job_tx, speaker, &mut buf, buf_start_secs, sample_rate, channels);
                return; // o job_tx deste canal morre aqui
            },
            recv(data_rx) -> chunk => match chunk {
                Ok(c) => {
                    let gap = last_data_at.map(|t| t.elapsed().as_secs_f32()).unwrap_or(0.0);
                    last_data_at = Some(Instant::now());
                    if !buf.is_empty() && gap > 0.5 {
                        dispatch(&job_tx, speaker, &mut buf, buf_start_secs, sample_rate, channels);
                    }
                    if buf.is_empty() {
                        let chunk_secs = c.len() as f32 / ch as f32 / sample_rate as f32;
                        buf_start_secs = (started.elapsed().as_secs_f32() - chunk_secs).max(0.0);
                    }
                    buf.extend(c);
                    // Procurar silêncio custa; a cada pacote de 10 ms seria
                    // desperdício. Reavaliamos a cada `CUT_CHECK_EVERY`.
                    let vencido = last_cut_check.is_none_or(|t: Instant| t.elapsed() >= CUT_CHECK_EVERY);
                    if vencido && buf.len() >= min_check_samples {
                        last_cut_check = Some(Instant::now());
                        if let Some(cut) = chunk::plan_cut(&buf, sample_rate, ch, chunk_opts) {
                            let piece: Vec<f32> = buf.drain(..cut.at.min(buf.len())).collect();
                            let secs = piece.len() as f32 / ch as f32 / sample_rate as f32;
                            if cut.reason == CutReason::Forced {
                                tracing::debug!(
                                    speaker = %speaker.label(),
                                    at = buf_start_secs + secs,
                                    "bloco cortado sem silêncio à vista"
                                );
                            }
                            let _ = job_tx.send(Job {
                                speaker,
                                offset: buf_start_secs,
                                audio: RawAudio {
                                    samples: piece,
                                    sample_rate,
                                    channels,
                                },
                                forced_cut: cut.reason == CutReason::Forced,
                                provisional: false,
                            });
                            buf_start_secs += secs;
                        }
                    }
                    // Legenda provisória: o buffer aberto vai ao Whisper de
                    // tempos em tempos, só com o worker livre — o bloco final
                    // substitui o texto em segundos.
                    let buf_secs = buf.len() as f32 / ch as f32 / sample_rate as f32;
                    let partial_due =
                        last_partial.is_none_or(|t: Instant| t.elapsed() >= PARTIAL_EVERY);
                    if partial_due && buf_secs >= PARTIAL_MIN_SECS && job_tx.is_empty() {
                        last_partial = Some(Instant::now());
                        let _ = job_tx.send(Job {
                            speaker,
                            offset: buf_start_secs,
                            audio: RawAudio {
                                samples: buf.clone(),
                                sample_rate,
                                channels,
                            },
                            forced_cut: false,
                            provisional: true,
                        });
                    }
                }
                // A fonte fechou o canal: só o `stop` encerra este loop, então
                // trata como stall (o loopback não tem reabertura por aqui —
                // dorme para não girar em vão até o stop chegar).
                Err(_) => {
                    if feed.stall_timeout().is_none() {
                        std::thread::sleep(Duration::from_millis(100));
                    } else {
                        recover(speaker, feed, &job_tx, &mut buf, buf_start_secs, &mut sample_rate, &mut channels, &mut last_data_at, &mut last_reopen_warn, stall);
                    }
                }
            },
            default(stall) => if feed.stall_timeout().is_some() {
                recover(speaker, feed, &job_tx, &mut buf, buf_start_secs, &mut sample_rate, &mut channels, &mut last_data_at, &mut last_reopen_warn, stall);
            },
        }
    }
}

/// Nada chegou por `stall`: o dispositivo sumiu (fone desconectado), foi
/// invalidado (suspensão) ou o driver travou. Despacha o que há no buffer e
/// reabre a fonte; se não der, tenta de novo no próximo stall — sem desistir
/// da reunião, que segue com o outro canal.
#[allow(clippy::too_many_arguments)]
fn recover(
    speaker: Speaker,
    feed: &mut dyn AudioFeed,
    job_tx: &Sender<Job>,
    buf: &mut Vec<f32>,
    buf_start_secs: f32,
    sample_rate: &mut u32,
    channels: &mut u16,
    last_data_at: &mut Option<Instant>,
    last_warn: &mut Option<Instant>,
    stall: Duration,
) {
    if !buf.is_empty() {
        let min = (0.3 * *sample_rate as f32) as usize * (*channels).max(1) as usize;
        if buf.len() >= min {
            let _ = job_tx.send(Job {
                speaker,
                offset: buf_start_secs,
                audio: RawAudio {
                    samples: std::mem::take(buf),
                    sample_rate: *sample_rate,
                    channels: *channels,
                },
                forced_cut: false,
                provisional: false,
            });
        }
        buf.clear();
    }
    match feed.reopen() {
        Ok(()) => {
            let (rate, ch) = feed.format();
            tracing::warn!(
                "{}: sem áudio por {:.0} s — captura reaberta ({rate} Hz, {ch} canal(is))",
                speaker.label(),
                stall.as_secs_f32()
            );
            *sample_rate = rate;
            *channels = ch;
            *last_data_at = None;
            *last_warn = None;
        }
        Err(e) => {
            let due = last_warn.is_none_or(|t| t.elapsed() >= REOPEN_WARN_EVERY);
            if due {
                tracing::warn!(
                    "{}: sem áudio por {:.0} s e não consegui reabrir ({e}) — tentando de novo",
                    speaker.label(),
                    stall.as_secs_f32()
                );
                *last_warn = Some(Instant::now());
            }
        }
    }
}

/// O que o worker de transcrição usa das opções (o resto é da captura).
struct TranscribeOptions {
    lang: String,
    initial_prompt: Option<String>,
    dictionary: Vec<String>,
    on_segment: Option<SegmentSink>,
    on_partial: Option<SegmentSink>,
    on_block: Option<BlockSink>,
}

/// Um canal gravado em disco durante a reunião. Se o arquivo não puder ser
/// criado, a reunião segue — só o passe final e a diarização ficam sem insumo.
struct ChannelSpool {
    spool: Option<PcmSpool>,
    speaker: Speaker,
}

impl ChannelSpool {
    fn create(speaker: Speaker) -> Self {
        match PcmSpool::create() {
            Ok(s) => Self {
                spool: Some(s),
                speaker,
            },
            Err(e) => {
                tracing::warn!(
                    canal = %speaker.label(),
                    "sem arquivo temporário para o áudio ({e}) — a transcrição final e a identificação de falantes ficam sem insumo nesta reunião"
                );
                Self {
                    spool: None,
                    speaker,
                }
            }
        }
    }

    fn push_at(&mut self, wall_start: f32, samples: &[f32]) {
        let Some(s) = self.spool.as_mut() else { return };
        if let Err(e) = s.push_at(wall_start, samples) {
            tracing::warn!(
                canal = %self.speaker.label(),
                "falha ao gravar o áudio em disco ({e}) — este canal fica sem passe final"
            );
            self.spool = None;
        }
    }

    fn finish(mut self, duration_secs: f32) -> ChannelAudio {
        let Some(mut s) = self.spool.take() else {
            return ChannelAudio::empty();
        };
        if let Err(e) = s.pad_to(duration_secs) {
            tracing::warn!(canal = %self.speaker.label(), "não consegui completar o áudio ({e})");
        }
        match s.finish() {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!(canal = %self.speaker.label(), "não consegui fechar o áudio ({e})");
                ChannelAudio::empty()
            }
        }
    }
}

/// Legenda provisória de um buffer ainda aberto: perfil mais barato, uma
/// linha só (os segmentos emendados), sem tocar na ata nem no arquivo do
/// passe final. Se já há bloco final esperando na fila, a provisória está
/// velha e nem vai ao Whisper.
#[allow(clippy::too_many_arguments)]
fn transcribe_partial(
    engine: &WhisperEngine,
    lang: &str,
    initial_prompt: Option<&str>,
    dictionary: &[String],
    on_partial: Option<&SegmentSink>,
    job_rx: &Receiver<Job>,
    speaker: Speaker,
    offset: f32,
    audio: RawAudio,
) {
    let Some(sink) = on_partial else { return };
    if !job_rx.is_empty() || audio.rms() < SILENCE_RMS {
        return;
    }
    let Ok(samples) = audio.into_whisper_input() else {
        return;
    };
    let decode = crate::profile::DecodeConfig::partial();
    let req = crate::engine::TranscribeRequest {
        lang,
        decode: &decode,
        prompt: initial_prompt,
    };
    match engine.transcribe_with(&samples, req) {
        Ok(t) => {
            let (Some(first), Some(last)) = (t.segments.first(), t.segments.last()) else {
                return;
            };
            let text = if dictionary.is_empty() {
                t.text.clone()
            } else {
                crate::text::apply_dictionary(&t.text, dictionary)
            };
            if text.trim().is_empty() {
                return;
            }
            sink(&MeetingSegment {
                speaker,
                start_secs: offset + first.start_secs,
                end_secs: offset + last.end_secs,
                text,
            });
        }
        Err(e) => tracing::debug!(speaker = %speaker.label(), "legenda provisória falhou: {e}"),
    }
}

/// Consome os blocos dos dois canais, transcreve ao vivo e grava o áudio dos
/// dois canais em disco para o passe final.
fn transcribe_worker(
    engine: Arc<WhisperEngine>,
    opts: TranscribeOptions,
    job_rx: Receiver<Job>,
    started: Instant,
) -> Result<MeetingResult> {
    let TranscribeOptions {
        lang,
        initial_prompt,
        dictionary,
        on_segment,
        on_partial,
        on_block,
    } = opts;
    let mut segments: Vec<MeetingSegment> = Vec::new();
    let mut forced_cuts = 0usize;
    let mut others = ChannelSpool::create(Speaker::Others);
    let mut me = ChannelSpool::create(Speaker::Me);

    for job in job_rx.iter() {
        let Job {
            speaker,
            offset,
            audio,
            forced_cut,
            provisional,
        } = job;
        if provisional {
            transcribe_partial(
                &engine,
                &lang,
                initial_prompt.as_deref(),
                &dictionary,
                on_partial.as_ref(),
                &job_rx,
                speaker,
                offset,
                audio,
            );
            continue;
        }
        let block_secs = audio.duration_secs();
        if forced_cut {
            forced_cuts += 1;
        }
        let quiet = audio.rms() < SILENCE_RMS;
        let samples = match audio.into_whisper_input() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("bloco falhou no resample: {e}");
                continue;
            }
        };
        // O áudio vai para o disco SEMPRE, silencioso ou não: é o silêncio
        // que mantém a linha do tempo do arquivo igual à da reunião.
        match speaker {
            Speaker::Me => me.push_at(offset, &samples),
            _ => others.push_at(offset, &samples),
        }
        if quiet {
            tracing::debug!(speaker = %speaker.label(), offset, "bloco silencioso não vai ao Whisper");
            continue;
        }

        match engine.transcribe(&samples, &lang, initial_prompt.as_deref()) {
            Ok(t) => {
                tracing::debug!(
                    speaker = %speaker.label(),
                    offset,
                    block_secs,
                    infer_secs = t.infer_secs,
                    forced_cut,
                    "bloco transcrito"
                );
                if let Some(sink) = &on_block {
                    sink(&BlockStats {
                        speaker,
                        block_secs,
                        infer_secs: Some(t.infer_secs),
                    });
                }
                for seg in t.segments {
                    if seg.text.is_empty() {
                        continue;
                    }
                    let text = if dictionary.is_empty() {
                        seg.text
                    } else {
                        crate::text::apply_dictionary(&seg.text, &dictionary)
                    };
                    let seg = MeetingSegment {
                        speaker,
                        start_secs: offset + seg.start_secs,
                        end_secs: offset + seg.end_secs,
                        text,
                    };
                    if let Some(sink) = &on_segment {
                        sink(&seg);
                    }
                    segments.push(seg);
                }
            }
            Err(e) => {
                tracing::warn!("bloco falhou: {e}");
                if let Some(sink) = &on_block {
                    sink(&BlockStats {
                        speaker,
                        block_secs,
                        infer_secs: None,
                    });
                }
            }
        }
    }
    segments.sort_by(|a, b| a.start_secs.total_cmp(&b.start_secs));
    let duration_secs = started.elapsed().as_secs_f32();
    let others_audio = others.finish(duration_secs);
    let me_audio = me.finish(duration_secs);
    if forced_cuts > 0 {
        tracing::info!(
            forced_cuts,
            "blocos cortados sem silêncio à vista — o passe final refaz esses trechos inteiros"
        );
    }
    Ok(MeetingResult {
        segments,
        duration_secs,
        others_audio,
        me_audio,
        forced_cuts,
    })
}

/// Gera o Markdown da reunião recém-gravada (sem resumo — ele é anexado
/// depois, se houver provider de IA).
pub fn to_markdown(
    title: &str,
    started_at: &str,
    result: &MeetingResult,
    moments: &[f32],
) -> String {
    let labels: Vec<String> = result.segments.iter().map(|s| s.speaker.label()).collect();
    let refs: Vec<SegmentRef<'_>> = result
        .segments
        .iter()
        .zip(&labels)
        .map(|(s, label)| SegmentRef {
            speaker: label,
            start_secs: s.start_secs,
            end_secs: s.end_secs,
            text: &s.text,
        })
        .collect();
    render_markdown(
        title,
        started_at,
        result.duration_secs,
        &refs,
        None,
        moments,
    )
}

/// Markdown completo a partir de segmentos quaisquer (recém-gravados ou do
/// banco): cabeçalho, um parágrafo por grupo de falas e, se houver, o resumo.
/// É a fonte única do formato — renomear falante ou título regrava o arquivo
/// por aqui.
pub fn render_markdown(
    title: &str,
    started_at: &str,
    duration_secs: f32,
    segments: &[SegmentRef<'_>],
    summary: Option<&str>,
    moments: &[f32],
) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {title}\n\n"));
    out.push_str(&format!(
        "> Transcrito 100% localmente pelo ISPer em {started_at} · duração {}.\n",
        fmt_ts(duration_secs)
    ));
    out.push_str("> Lembrete (LGPD): avise os participantes de que a reunião foi transcrita.\n");
    for g in group_speech(segments.iter().copied()) {
        out.push_str(&format!(
            "\n**[{}] {}:** {}\n",
            fmt_ts(g.start_secs),
            g.speaker,
            g.text
        ));
    }
    if !moments.is_empty() {
        out.push_str("\n## Momentos marcados\n\n");
        for (at, excerpt) in moment_excerpts(segments, moments) {
            out.push_str(&format!("- **[{}]** {}\n", fmt_ts(at), excerpt));
        }
    }
    if let Some(summary) = summary.map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str("\n---\n\n");
        out.push_str(summary);
        out.push_str("\n\n> Resumo gerado por IA — revise antes de usar.\n");
    }
    out
}

/// Para cada momento marcado (em ordem), o parágrafo que estava em curso: o
/// último que começa até meio segundo depois do instante — quem marca costuma
/// reagir ao que acabou de ouvir. Texto encurtado para caber numa linha.
pub fn moment_excerpts(segments: &[SegmentRef<'_>], moments: &[f32]) -> Vec<(f32, String)> {
    const MAX_CHARS: usize = 160;
    let groups = group_speech(segments.iter().copied());
    let mut sorted: Vec<f32> = moments.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    sorted
        .into_iter()
        .map(|at| {
            let group = groups
                .iter()
                .rfind(|g| g.start_secs <= at + 0.5)
                .or(groups.first());
            let excerpt = match group {
                Some(g) => {
                    let mut text: String = g.text.chars().take(MAX_CHARS).collect();
                    if g.text.chars().count() > MAX_CHARS {
                        text.push('…');
                    }
                    format!("{}: {}", g.speaker, text)
                }
                None => "(sem fala transcrita neste instante)".to_string(),
            };
            (at, excerpt)
        })
        .collect()
}

/// `mm:ss`, ou `h:mm:ss` a partir de uma hora.
pub fn fmt_ts(secs: f32) -> String {
    let s = secs.max(0.0) as u32;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(speaker: &'static str, start: f32, end: f32, text: &'static str) -> SegmentRef<'static> {
        SegmentRef {
            speaker,
            start_secs: start,
            end_secs: end,
            text,
        }
    }

    /// Fonte de mentira para o fatiador: o teste controla o canal e observa
    /// as reaberturas. Só a primeira reabertura troca o canal (as seguintes,
    /// enquanto nada chega, só contam) — assim o teste sabe para onde enviar.
    struct FakeFeed {
        rx: Receiver<Vec<f32>>,
        shared: Arc<std::sync::Mutex<FakeShared>>,
    }

    struct FakeShared {
        tx: Sender<Vec<f32>>,
        reopens: usize,
        closed: bool,
    }

    impl AudioFeed for FakeFeed {
        fn receiver(&self) -> &Receiver<Vec<f32>> {
            &self.rx
        }
        fn format(&self) -> (u32, u16) {
            // 100 Hz: o mínimo despachável (0,3 s) são 30 amostras, e os
            // carimbos de tempo saem do relógio real do teste.
            (100, 1)
        }
        fn stall_timeout(&self) -> Option<Duration> {
            Some(Duration::from_millis(80))
        }
        fn reopen(&mut self) -> Result<()> {
            let mut shared = self.shared.lock().unwrap();
            if shared.reopens == 0 {
                let (tx, rx) = unbounded();
                self.rx = rx;
                shared.tx = tx;
            }
            shared.reopens += 1;
            Ok(())
        }
        fn close(&mut self) {
            self.shared.lock().unwrap().closed = true;
        }
    }

    #[test]
    fn fonte_que_para_de_entregar_e_reaberta_e_a_reuniao_continua() {
        let started = Instant::now();
        let (job_tx, job_rx) = unbounded::<Job>();
        let (stop_tx, stop_rx) = unbounded::<()>();
        let (tx0, rx0) = unbounded();
        let shared = Arc::new(std::sync::Mutex::new(FakeShared {
            tx: tx0,
            reopens: 0,
            closed: false,
        }));
        let mut feed = FakeFeed {
            rx: rx0,
            shared: shared.clone(),
        };
        let worker = std::thread::spawn(move || {
            chunk_loop(Speaker::Me, started, stop_rx, job_tx, &mut feed)
        });

        // 0,4 s de áudio chega logo no início… e depois nada: o "fone caiu".
        shared.lock().unwrap().tx.send(vec![0.5; 40]).unwrap();
        std::thread::sleep(Duration::from_millis(400));
        assert!(
            shared.lock().unwrap().reopens >= 1,
            "o stall deve reabrir a fonte"
        );
        // O que estava no buffer foi despachado antes de reabrir, carimbado
        // no início da reunião (chegou antes de 0,4 s de relógio → 0).
        let job = job_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(job.speaker, Speaker::Me);
        assert_eq!(job.audio.samples.len(), 40);
        assert_eq!((job.audio.sample_rate, job.audio.channels), (100, 1));
        assert_eq!(job.offset, 0.0);

        // Pelo canal novo o áudio volta a fluir, carimbado pelo relógio: 0,3 s
        // de áudio chegando com ≥ 0,4 s de reunião → começa em ≥ 0,1 s.
        let tx = shared.lock().unwrap().tx.clone();
        tx.send(vec![0.25; 30]).unwrap();
        std::thread::sleep(Duration::from_millis(30));
        stop_tx.send(()).unwrap();
        worker.join().unwrap();
        let job2 = job_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(job2.audio.samples.len(), 30);
        assert!(job2.offset >= 0.09, "{} deveria ser ≥ 0,1 s", job2.offset);
        assert!(shared.lock().unwrap().closed, "o stop fecha a fonte");
        assert!(job_rx.try_recv().is_err(), "nada além dos dois blocos");
    }

    #[test]
    fn audio_dos_participantes_vai_para_o_disco_e_volta_igual() {
        let mut spool = PcmSpool::create().unwrap();
        let path = spool.path.clone();
        spool
            .push_at(0.0, &[0.0, 0.5, -0.5, 1.0, -1.0, 2.0, -2.0])
            .unwrap();
        spool.push_at(7.0 / 16_000.0, &vec![0.25; 16_000]).unwrap();
        let audio = spool.finish().unwrap();
        assert!(path.is_file(), "o arquivo existe enquanto o handle vive");
        assert_eq!(audio.samples(), 16_007);
        assert!((audio.secs() - 16_007.0 / 16_000.0).abs() < 1e-6);

        let back = audio.read_f32().unwrap();
        assert_eq!(back.len(), 16_007);
        // Fora de [-1, 1] satura; dentro, erro de quantização de 16 bits.
        let expected = [0.0, 0.5, -0.5, 1.0, -1.0, 1.0, -1.0];
        for (got, want) in back.iter().zip(expected) {
            assert!((got - want).abs() < 1.0 / 32_000.0, "{got} vs {want}");
        }
        assert!((back[7] - 0.25).abs() < 1.0 / 32_000.0);

        drop(audio);
        assert!(!path.exists(), "o arquivo temporário some com o handle");
    }

    #[test]
    fn leitura_em_blocos_nao_parte_amostra_e_vazio_e_vazio() {
        // 100.001 amostras = 200.002 bytes: maior que o buffer de leitura
        // (64 KiB) e não múltiplo dele — a amostra partida entre dois blocos
        // tem de ser remontada.
        let data: Vec<f32> = (0..100_001)
            .map(|i| (i % 2000) as f32 / 1000.0 - 1.0)
            .collect();
        let mut spool = PcmSpool::create().unwrap();
        spool.push_at(0.0, &data).unwrap();
        let audio = spool.finish().unwrap();
        let back = audio.read_f32().unwrap();
        assert_eq!(back.len(), data.len());
        for (got, want) in back.iter().zip(&data) {
            assert!((got - want).abs() < 1.0 / 32_000.0, "{got} vs {want}");
        }

        let empty = ChannelAudio::empty();
        assert!(empty.is_empty());
        assert_eq!(empty.secs(), 0.0);
        assert!(empty.read_f32().unwrap().is_empty());
    }

    #[test]
    fn buraco_na_entrega_vira_silencio_e_a_posicao_continua_sendo_o_relogio() {
        // O canal entrega 0,5 s no instante 0 e só volta no instante 10: sem
        // o preenchimento, os dois trechos ficariam colados e a diarização
        // veria uma troca de voz que não houve.
        let mut spool = PcmSpool::create().unwrap();
        spool.push_at(0.0, &vec![0.4; 8_000]).unwrap();
        spool.push_at(10.0, &vec![-0.4; 8_000]).unwrap();
        spool.pad_to(12.0).unwrap();
        let audio = spool.finish().unwrap();
        assert_eq!(audio.samples(), 12 * 16_000);
        let back = audio.read_f32().unwrap();
        // O segundo trecho está exatamente aos 10 s.
        assert!((back[10 * 16_000] + 0.4).abs() < 1.0 / 32_000.0);
        // E o buraco é silêncio de verdade.
        assert!(back[16_000..10 * 16_000].iter().all(|s| *s == 0.0));
    }

    #[test]
    fn spool_nao_retrocede_quando_o_relogio_volta() {
        // Jitter de relógio não pode fazer o arquivo encolher nem sobrescrever
        // o que já foi gravado.
        let mut spool = PcmSpool::create().unwrap();
        spool.push_at(5.0, &vec![0.4; 1_600]).unwrap();
        spool.push_at(1.0, &vec![0.2; 1_600]).unwrap();
        let audio = spool.finish().unwrap();
        assert_eq!(audio.samples(), 5 * 16_000 + 3_200);
    }

    #[test]
    fn spool_descartado_sem_finish_nao_deixa_lixo() {
        let spool = PcmSpool::create().unwrap();
        let path = spool.path.clone();
        assert!(path.is_file());
        drop(spool);
        assert!(!path.exists());
    }

    #[test]
    fn agrupa_mesmo_falante_com_pausa_curta() {
        let g = group_speech([seg("Eu", 0.0, 2.0, "Oi,"), seg("Eu", 2.5, 4.0, "tudo bem?")]);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].text, "Oi, tudo bem?");
        assert_eq!(g[0].end_secs, 4.0);
    }

    #[test]
    fn separa_por_falante_pausa_longa_e_paragrafo_grande() {
        let g = group_speech([
            seg("Eu", 0.0, 2.0, "a"),
            seg("Participantes", 2.0, 3.0, "b"),  // outro falante
            seg("Participantes", 8.0, 9.0, "c"),  // pausa de 5 s > GROUP_GAP_SECS
            seg("Participantes", 9.5, 70.0, "d"), // passaria de 60 s no grupo
        ]);
        assert_eq!(g.len(), 4);
        assert_eq!(g[3].start_secs, 9.5);
    }

    #[test]
    fn ignora_texto_vazio_e_formata_tempo() {
        assert!(group_speech([seg("Eu", 0.0, 1.0, "   ")]).is_empty());
        assert_eq!(fmt_ts(65.0), "01:05");
        assert_eq!(fmt_ts(4587.0), "1:16:27");
    }

    #[test]
    fn momentos_marcados_viram_secao_ordenada_com_trecho() {
        let segs = [
            seg("Eu", 0.0, 3.0, "Vamos começar pelo plano."),
            seg(
                "Participante 1",
                10.0,
                14.0,
                "O MRP precisa de revisão de lead time.",
            ),
        ];
        let md = render_markdown("R", "09/09/2026", 20.0, &segs, None, &[12.0, 1.0]);
        assert!(md.contains(
            "## Momentos marcados\n\n\
             - **[00:01]** Eu: Vamos começar pelo plano.\n\
             - **[00:12]** Participante 1: O MRP precisa de revisão de lead time.\n"
        ));
        // Marcado antes de qualquer fala: usa o primeiro parágrafo.
        let early = moment_excerpts(&segs, &[0.0]);
        assert!(early[0].1.starts_with("Eu: Vamos"));
        // Sem momentos, sem seção.
        assert!(!render_markdown("R", "x", 1.0, &segs, None, &[]).contains("Momentos marcados"));
    }

    #[test]
    fn markdown_tem_paragrafos_e_resumo() {
        let md = render_markdown(
            "Reunião X",
            "09/09/2026 10:00",
            125.0,
            &[
                seg("Eu", 0.0, 1.0, "Olá."),
                seg("Participante 1", 5.0, 6.0, "Oi."),
            ],
            Some("## Resumo\nCurto."),
            &[],
        );
        assert!(md.starts_with("# Reunião X\n"));
        assert!(md.contains("**[00:00] Eu:** Olá.\n"));
        assert!(md.contains("**[00:05] Participante 1:** Oi.\n"));
        assert!(md.contains("---\n\n## Resumo\nCurto.\n\n> Resumo gerado por IA"));
    }
}
