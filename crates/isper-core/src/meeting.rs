//! Gravador de reuniões (Fase 4): microfone + áudio do sistema (loopback
//! WASAPI), transcritos em blocos DURANTE a gravação.
//!
//! Sem bot e sem API paga: o Windows deixa capturar o que sai da caixa de
//! som (loopback — módulo [`crate::loopback`]). Quem veio do mic é "Eu";
//! quem veio do loopback é "Participantes". Cada canal é fatiado em blocos
//! de ~20 s — cortados num ponto de silêncio para não partir palavra — e
//! transcrito em paralelo à gravação: ao encerrar, só o resto é processado.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};

use crate::audio::RawAudio;
use crate::loopback::LoopbackSource;
use crate::{IsperError, Result, WhisperEngine, audio, loopback};

/// Recebe cada fala assim que o bloco dela é transcrito — é a transcrição
/// "ao vivo": chega com o atraso de um bloco (~20 s) + a inferência.
pub type SegmentSink = Arc<dyn Fn(&MeetingSegment) + Send + Sync>;

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

/// Tamanho alvo de cada bloco de transcrição.
const CHUNK_SECS: f32 = 20.0;
/// Blocos mais silenciosos que isso nem vão para o Whisper (economiza GPU e
/// evita as alucinações clássicas em silêncio, tipo "Legendas pela...").
const SILENCE_RMS: f32 = 0.0035;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
    Me,
    /// Alguém do loopback, sem identificação individual.
    Others,
    /// Alguém do loopback identificado pela diarização (1, 2, 3…).
    Participant(u8),
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

/// Um bloco de áudio dos participantes já em 16 kHz: onde começa no relógio
/// da reunião e onde caiu no áudio concatenado guardado p/ diarização.
#[derive(Debug, Clone, Copy)]
pub struct AudioBlock {
    pub wall_start: f32,
    pub concat_start: f32,
    pub secs: f32,
}

#[derive(Debug, Clone)]
pub struct MeetingSegment {
    pub speaker: Speaker,
    pub start_secs: f32,
    pub end_secs: f32,
    pub text: String,
}

pub struct MeetingResult {
    /// Segmentos dos dois canais, em ordem cronológica.
    pub segments: Vec<MeetingSegment>,
    pub duration_secs: f32,
    /// Áudio dos participantes (16 kHz mono, PCM 16 bits) concatenado — insumo
    /// da diarização, que roda depois, fora do core (crate `isper-diarize`).
    /// Em i16 para caber na memória em reuniões longas: 1 h = 115 MB (em f32
    /// seria o dobro); [`Self::others_audio_f32`] converte na hora de usar.
    pub others_audio_16k: Vec<i16>,
    /// Mapa bloco a bloco entre o áudio concatenado e o relógio da reunião.
    pub others_blocks: Vec<AudioBlock>,
}

/// Tempo no relógio da reunião → posição no áudio concatenado.
fn wall_to_concat(blocks: &[AudioBlock], t: f32) -> Option<f32> {
    blocks
        .iter()
        .find(|b| t >= b.wall_start && t <= b.wall_start + b.secs)
        .map(|b| b.concat_start + (t - b.wall_start))
}

impl MeetingResult {
    /// Áudio dos participantes em f32 normalizado, como a diarização espera.
    pub fn others_audio_f32(&self) -> Vec<f32> {
        self.others_audio_16k
            .iter()
            .map(|s| *s as f32 / i16::MAX as f32)
            .collect()
    }

    /// Aplica turnos de falante (em tempo do áudio concatenado, como a
    /// diarização devolve) aos segmentos "Participantes": cada segmento vira
    /// "Participante N" do turno com maior sobreposição.
    pub fn apply_speaker_turns(&mut self, turns: &[(f32, f32, usize)]) {
        // Renumera os falantes por ordem de aparição: os ids do agrupamento
        // podem ter buracos (clusters minúsculos são filtrados) e o leitor
        // espera "Participante 1, 2, 3…".
        let mut order: Vec<(f32, usize)> = turns.iter().map(|(s, _, spk)| (*s, *spk)).collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut renumber: Vec<(usize, u8)> = Vec::new();
        for (_, spk) in order {
            if !renumber.iter().any(|(id, _)| *id == spk) {
                let next = (renumber.len() + 1).min(255) as u8;
                renumber.push((spk, next));
            }
        }
        let number_of = |spk: usize| -> u8 {
            renumber
                .iter()
                .find(|(id, _)| *id == spk)
                .map(|(_, n)| *n)
                .unwrap_or(1)
        };

        // Empréstimos disjuntos: lemos `others_blocks` enquanto mutamos `segments`.
        let blocks = &self.others_blocks;
        for seg in self.segments.iter_mut() {
            if seg.speaker != Speaker::Others {
                continue;
            }
            let Some(cs) = wall_to_concat(blocks, seg.start_secs) else {
                continue;
            };
            let ce = cs + (seg.end_secs - seg.start_secs).max(0.1);
            let best = turns
                .iter()
                .map(|(s, e, spk)| ((e.min(ce) - s.max(cs)).max(0.0), *spk))
                .filter(|(overlap, _)| *overlap > 0.0)
                .max_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, spk)) = best {
                seg.speaker = Speaker::Participant(number_of(spk));
            }
        }
    }

    /// Quantos participantes distintos foram identificados.
    pub fn distinct_participants(&self) -> usize {
        let mut ids: Vec<u8> = self
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

type Job = (Speaker, f32, RawAudio);

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

    std::thread::spawn(move || {
        let result = transcribe_worker(
            engine,
            lang,
            initial_prompt,
            on_segment,
            dictionary,
            job_rx,
            started,
        );
        let _ = done_tx.send(result);
    });

    Ok(MeetingHandle {
        stop_txs,
        done_rx,
        started,
        warnings,
    })
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
    use cpal::traits::StreamTrait;

    match speaker {
        // "Eu": microfone via cpal, como no ditado.
        Speaker::Me => {
            let (data_tx, data_rx) = unbounded();
            let (stream, sample_rate, channels) =
                match audio::open_input_stream_on(input_device.as_deref(), data_tx) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
            if let Err(e) = stream.play() {
                let _ = ready_tx.send(Err(IsperError::Audio(e.to_string())));
                return;
            }
            let _ = ready_tx.send(Ok(None));
            chunk_loop(
                speaker,
                started,
                stop_rx,
                data_rx,
                sample_rate,
                channels,
                job_tx,
                move || drop(stream), // dropar o stream encerra a captura
            );
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
            let (sample_rate, channels) = (ready.sample_rate, ready.channels);
            let _ = ready_tx.send(Ok(ready.warning));
            chunk_loop(
                speaker,
                started,
                stop_rx,
                data_rx,
                sample_rate,
                channels,
                job_tx,
                move || {
                    let _ = pump_stop_tx.send(());
                },
            );
        }
    }
}

/// Acumula amostras de um canal e despacha blocos para transcrição.
#[allow(clippy::too_many_arguments)]
fn chunk_loop(
    speaker: Speaker,
    started: Instant,
    stop_rx: Receiver<()>,
    data_rx: Receiver<Vec<f32>>,
    sample_rate: u32,
    channels: u16,
    job_tx: Sender<Job>,
    on_stop: impl FnOnce(),
) {
    let ch = channels.max(1) as usize;
    let chunk_samples = (CHUNK_SECS * sample_rate as f32) as usize * ch;
    let mut buf: Vec<f32> = Vec::with_capacity(chunk_samples + sample_rate as usize * ch);
    // Offset global (em segundos) do início do buffer atual. Marcado pelo
    // RELÓGIO da reunião quando o buffer começa a encher: o loopback só
    // entrega amostras enquanto algo está tocando, então contar amostras
    // subestimaria o tempo real durante as pausas.
    let mut buf_start_secs = 0.0f32;
    // Se a entrega parar por >0,5 s (pausa na reprodução), despachamos o
    // buffer e recarimbamos — cada trecho cai no timestamp certo mesmo com
    // entrega intermitente.
    let mut last_data_at: Option<Instant> = None;

    loop {
        crossbeam_channel::select! {
            recv(stop_rx) -> _ => {
                on_stop();
                std::thread::sleep(Duration::from_millis(60));
                for c in data_rx.try_iter() {
                    buf.extend(c);
                }
                let min = (0.3 * sample_rate as f32) as usize * ch;
                if buf.len() >= min {
                    let _ = job_tx.send((speaker, buf_start_secs, RawAudio {
                        samples: std::mem::take(&mut buf),
                        sample_rate,
                        channels,
                    }));
                }
                return; // o job_tx deste canal morre aqui
            },
            recv(data_rx) -> chunk => if let Ok(c) = chunk {
                let gap = last_data_at.map(|t| t.elapsed().as_secs_f32()).unwrap_or(0.0);
                last_data_at = Some(Instant::now());
                if !buf.is_empty() && gap > 0.5 {
                    let piece: Vec<f32> = std::mem::take(&mut buf);
                    let _ = job_tx.send((speaker, buf_start_secs, RawAudio {
                        samples: piece,
                        sample_rate,
                        channels,
                    }));
                }
                if buf.is_empty() {
                    let chunk_secs = c.len() as f32 / ch as f32 / sample_rate as f32;
                    buf_start_secs = (started.elapsed().as_secs_f32() - chunk_secs).max(0.0);
                }
                buf.extend(c);
                if buf.len() >= chunk_samples {
                    let cut = quiet_cut(&buf, sample_rate, ch);
                    let piece: Vec<f32> = buf.drain(..cut).collect();
                    let secs = piece.len() as f32 / ch as f32 / sample_rate as f32;
                    let _ = job_tx.send((speaker, buf_start_secs, RawAudio {
                        samples: piece,
                        sample_rate,
                        channels,
                    }));
                    buf_start_secs += secs;
                }
            },
        }
    }
}

/// Acha um ponto de corte silencioso: a janela de 100 ms com menor energia
/// dentro do último 1,5 s do buffer. Cortar no silêncio evita partir uma
/// palavra entre dois blocos.
fn quiet_cut(buf: &[f32], sample_rate: u32, ch: usize) -> usize {
    let frames = buf.len() / ch;
    let win = (sample_rate as usize / 10).max(1); // 100 ms
    let search = ((sample_rate as usize) * 3 / 2).min(frames); // último 1,5 s
    if search < win * 2 {
        return buf.len();
    }
    let start = frames - search;
    let mut best_frame = frames;
    let mut best_energy = f32::MAX;
    let mut f = start;
    while f + win <= frames {
        let mut e = 0.0f32;
        for fr in f..f + win {
            let mut s = 0.0f32;
            for c in 0..ch {
                s += buf[fr * ch + c];
            }
            let m = s / ch as f32;
            e += m * m;
        }
        if e < best_energy {
            best_energy = e;
            best_frame = f + win / 2;
        }
        f += win / 2; // passos de 50 ms
    }
    best_frame * ch
}

/// Consome os blocos dos dois canais e monta a lista final de segmentos.
fn transcribe_worker(
    engine: Arc<WhisperEngine>,
    lang: String,
    initial_prompt: Option<String>,
    on_segment: Option<SegmentSink>,
    dictionary: Vec<String>,
    job_rx: Receiver<Job>,
    started: Instant,
) -> Result<MeetingResult> {
    let mut segments: Vec<MeetingSegment> = Vec::new();
    let mut others_audio_16k: Vec<i16> = Vec::new();
    let mut others_blocks: Vec<AudioBlock> = Vec::new();
    for (speaker, offset, raw) in job_rx.iter() {
        let block_secs = raw.duration_secs();
        if raw.rms() < SILENCE_RMS {
            tracing::debug!(speaker = %speaker.label(), offset, "bloco silencioso pulado");
            continue;
        }
        let samples = match raw.into_whisper_input() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("bloco falhou no resample: {e}");
                continue;
            }
        };
        if speaker == Speaker::Others {
            others_blocks.push(AudioBlock {
                wall_start: offset,
                concat_start: others_audio_16k.len() as f32 / crate::WHISPER_SAMPLE_RATE as f32,
                secs: samples.len() as f32 / crate::WHISPER_SAMPLE_RATE as f32,
            });
            others_audio_16k.extend(
                samples
                    .iter()
                    .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16),
            );
        }
        match engine.transcribe(&samples, &lang, initial_prompt.as_deref()) {
            Ok(t) => {
                tracing::info!(
                    speaker = %speaker.label(),
                    offset,
                    block_secs,
                    infer_secs = t.infer_secs,
                    "bloco transcrito"
                );
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
            Err(e) => tracing::warn!("bloco falhou: {e}"),
        }
    }
    segments.sort_by(|a, b| a.start_secs.total_cmp(&b.start_secs));
    Ok(MeetingResult {
        segments,
        duration_secs: started.elapsed().as_secs_f32(),
        others_audio_16k,
        others_blocks,
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
                .filter(|g| g.start_secs <= at + 0.5)
                .last()
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
