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

use crossbeam_channel::{unbounded, Receiver, Sender};

use crate::audio::RawAudio;
use crate::loopback::LoopbackSource;
use crate::{audio, loopback, IsperError, Result, WhisperEngine};

/// Opções de uma gravação de reunião.
#[derive(Debug, Clone)]
pub struct MeetingOptions {
    pub lang: String,
    pub initial_prompt: Option<String>,
    /// De onde vem o áudio dos "Participantes" (sistema inteiro ou só um app).
    pub source: LoopbackSource,
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
    /// Áudio dos participantes (16 kHz mono) concatenado — insumo da
    /// diarização, que roda depois, fora do core (crate `isper-diarize`).
    pub others_audio_16k: Vec<f32>,
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
        std::thread::spawn(move || {
            capture_channel(speaker, source, started, stop_rx, job_tx, ready_tx)
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
                return Err(IsperError::Audio("timeout abrindo captura da reunião".into()));
            }
        }
    }
    // O worker termina quando os DOIS capturadores largarem o canal de jobs.
    drop(job_tx);

    std::thread::spawn(move || {
        let result = transcribe_worker(engine, lang, initial_prompt, job_rx, started);
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
            let (stream, sample_rate, channels) = match audio::open_input_stream(data_tx) {
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
    job_rx: Receiver<Job>,
    started: Instant,
) -> Result<MeetingResult> {
    let mut segments: Vec<MeetingSegment> = Vec::new();
    let mut others_audio_16k: Vec<f32> = Vec::new();
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
            others_audio_16k.extend_from_slice(&samples);
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
                    segments.push(MeetingSegment {
                        speaker,
                        start_secs: offset + seg.start_secs,
                        end_secs: offset + seg.end_secs,
                        text: seg.text,
                    });
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

/// Gera o Markdown da reunião, agrupando falas consecutivas do mesmo falante.
pub fn to_markdown(title: &str, started_at: &str, result: &MeetingResult) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {title}\n\n"));
    out.push_str(&format!(
        "> Transcrito 100% localmente pelo ISPer em {started_at} · duração {}.\n",
        fmt_ts(result.duration_secs)
    ));
    out.push_str("> Lembrete (LGPD): avise os participantes de que a reunião foi transcrita.\n");

    let mut last: Option<Speaker> = None;
    for seg in &result.segments {
        if last != Some(seg.speaker) {
            out.push_str(&format!(
                "\n**[{}] {}:** ",
                fmt_ts(seg.start_secs),
                seg.speaker.label()
            ));
            last = Some(seg.speaker);
        } else {
            out.push(' ');
        }
        out.push_str(&seg.text);
    }
    out.push('\n');
    out
}

fn fmt_ts(secs: f32) -> String {
    let s = secs.max(0.0) as u32;
    format!("{:02}:{:02}", s / 60, s % 60)
}
