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
use crate::{audio, loopback, IsperError, Result, WhisperEngine};

/// Tamanho alvo de cada bloco de transcrição.
const CHUNK_SECS: f32 = 20.0;
/// Blocos mais silenciosos que isso nem vão para o Whisper (economiza GPU e
/// evita as alucinações clássicas em silêncio, tipo "Legendas pela...").
const SILENCE_RMS: f32 = 0.0035;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
    Me,
    Others,
}

impl Speaker {
    pub fn label(&self) -> &'static str {
        match self {
            Speaker::Me => "Eu",
            Speaker::Others => "Participantes",
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
    /// Segmentos dos dois canais, em ordem cronológica.
    pub segments: Vec<MeetingSegment>,
    pub duration_secs: f32,
}

pub struct MeetingHandle {
    stop_txs: Vec<Sender<()>>,
    done_rx: Receiver<Result<MeetingResult>>,
    started: Instant,
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
/// `lang` e `initial_prompt` seguem para todas as transcrições da reunião.
pub fn start(
    engine: Arc<WhisperEngine>,
    lang: String,
    initial_prompt: Option<String>,
) -> Result<MeetingHandle> {
    let (job_tx, job_rx) = unbounded::<Job>();
    let (done_tx, done_rx) = unbounded();
    let (ready_tx, ready_rx) = unbounded::<Result<()>>();
    let started = Instant::now();

    let mut stop_txs = Vec::new();
    for speaker in [Speaker::Me, Speaker::Others] {
        let (stop_tx, stop_rx) = unbounded::<()>();
        stop_txs.push(stop_tx);
        let job_tx = job_tx.clone();
        let ready_tx = ready_tx.clone();
        std::thread::spawn(move || capture_channel(speaker, started, stop_rx, job_tx, ready_tx));
    }
    // O worker termina quando os DOIS capturadores largarem o canal de jobs.
    drop(job_tx);

    // Espera os dois canais confirmarem que abriram.
    for _ in 0..2 {
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => {}
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

    std::thread::spawn(move || {
        let result = transcribe_worker(engine, lang, initial_prompt, job_rx, started);
        let _ = done_tx.send(result);
    });

    Ok(MeetingHandle {
        stop_txs,
        done_rx,
        started,
    })
}

/// Abre a fonte certa para o canal e roda o fatiador de blocos.
fn capture_channel(
    speaker: Speaker,
    started: Instant,
    stop_rx: Receiver<()>,
    job_tx: Sender<Job>,
    ready_tx: Sender<Result<()>>,
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
            let _ = ready_tx.send(Ok(()));
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
        // "Participantes": loopback via wasapi (polling), numa thread própria.
        Speaker::Others => {
            let (pump_stop_tx, pump_stop_rx) = unbounded::<()>();
            let (data_tx, data_rx) = unbounded();
            let (lb_ready_tx, lb_ready_rx) = unbounded();
            std::thread::spawn(move || loopback::run(pump_stop_rx, data_tx, lb_ready_tx));
            let (sample_rate, channels) = match lb_ready_rx.recv_timeout(Duration::from_secs(5)) {
                Ok(Ok(meta)) => meta,
                Ok(Err(e)) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
                Err(_) => {
                    let _ = ready_tx.send(Err(IsperError::Audio("timeout no loopback".into())));
                    return;
                }
            };
            let _ = ready_tx.send(Ok(()));
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
    for (speaker, offset, raw) in job_rx.iter() {
        let block_secs = raw.duration_secs();
        if raw.rms() < SILENCE_RMS {
            tracing::debug!(speaker = speaker.label(), offset, "bloco silencioso pulado");
            continue;
        }
        match raw
            .into_whisper_input()
            .and_then(|s| engine.transcribe(&s, &lang, initial_prompt.as_deref()))
        {
            Ok(t) => {
                tracing::info!(
                    speaker = speaker.label(),
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
