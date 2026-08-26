//! Gravador controlável por comandos — push-to-talk e mãos-livres (Fase 2).
//!
//! O `cpal::Stream` não é `Send`: ele precisa viver inteiro numa única
//! thread. Por isso este módulo sobe uma *thread de áudio* dedicada, dona do
//! stream, e conversa com o resto do app só por canais:
//!
//! - `start()` / `stop()` / `set_vad()` chegam por um canal de comandos;
//! - gravações concluídas saem como [`RecorderEvent::Finished`] — tanto as
//!   paradas manualmente quanto as encerradas pelo VAD;
//! - o nível do microfone (RMS) sai continuamente por um canal de níveis,
//!   que a UI usa para desenhar o waveform.
//!
//! O VAD aqui é por **energia** (RMS com piso de ruído adaptativo) — simples
//! e suficiente para detectar "parou de falar" em ambiente normal. Um VAD
//! neural (Silero) entra na Fase 4, onde reuniões pedem mais precisão.

use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, unbounded, Receiver, Sender};

use crate::{audio, IsperError, RawAudio, Result};

/// Silêncio contínuo (depois de ter ouvido fala) que encerra o mãos-livres.
const VAD_SILENCE: Duration = Duration::from_millis(1200);
/// Sem NENHUMA fala por esse tempo, o mãos-livres desiste sozinho.
const VAD_NO_SPEECH_TIMEOUT: Duration = Duration::from_secs(15);
/// Trava de segurança: nenhuma gravação passa disso.
const MAX_RECORDING: Duration = Duration::from_secs(120);

enum Command {
    Start,
    Stop,
    SetVad(bool),
}

pub enum RecorderEvent {
    /// A gravação terminou (Stop manual, VAD ou trava de tempo) — o áudio
    /// completo vem aqui. Erros de captura também chegam por este evento.
    Finished(Result<RawAudio>),
}

/// Alça para a thread de áudio. `Send + Sync` — pode viver no estado do Tauri.
pub struct AudioHandle {
    cmd_tx: Sender<Command>,
    event_rx: Receiver<RecorderEvent>,
    level_rx: Receiver<f32>,
}

impl AudioHandle {
    /// Começa a gravar (não bloqueia). Se já estiver gravando, é ignorado.
    pub fn start(&self) {
        let _ = self.cmd_tx.send(Command::Start);
    }

    /// Para de gravar; o áudio chega depois como [`RecorderEvent::Finished`].
    pub fn stop(&self) {
        let _ = self.cmd_tx.send(Command::Stop);
    }

    /// Liga/desliga o auto-stop por silêncio (modo mãos-livres).
    pub fn set_vad(&self, on: bool) {
        let _ = self.cmd_tx.send(Command::SetVad(on));
    }

    /// Canal de gravações concluídas — consuma numa thread própria.
    pub fn events(&self) -> Receiver<RecorderEvent> {
        self.event_rx.clone()
    }

    /// Canal com o nível RMS do microfone (~100 leituras/s durante a gravação).
    pub fn levels(&self) -> Receiver<f32> {
        self.level_rx.clone()
    }
}

/// Sobe a thread de áudio e devolve a alça de controle.
pub fn spawn() -> AudioHandle {
    let (cmd_tx, cmd_rx) = unbounded();
    let (event_tx, event_rx) = unbounded();
    // Bounded + try_send: se a UI não consumir os níveis a tempo, descartamos
    // leituras em vez de acumular memória.
    let (level_tx, level_rx) = bounded(64);
    std::thread::spawn(move || run(cmd_rx, event_tx, level_tx));
    AudioHandle {
        cmd_tx,
        event_rx,
        level_rx,
    }
}

/// Detector de fim de fala por energia.
struct Vad {
    enabled: bool,
    noise_floor: f32,
    voiced_run: u32,
    heard_speech: bool,
    last_voice: Instant,
}

impl Vad {
    fn new() -> Self {
        Self {
            enabled: false,
            noise_floor: 0.0,
            voiced_run: 0,
            heard_speech: false,
            last_voice: Instant::now(),
        }
    }

    /// Processa o RMS de um bloco (~10 ms). Devolve `true` quando é hora de
    /// encerrar a gravação.
    fn should_stop(&mut self, rms: f32, started: Instant) -> bool {
        if !self.enabled {
            return false;
        }
        // Piso de ruído adaptativo: desce na hora, sobe bem devagar (assim a
        // própria fala não infla o piso).
        if self.noise_floor == 0.0 || rms < self.noise_floor {
            self.noise_floor = rms.max(1e-4);
        } else {
            self.noise_floor += (rms - self.noise_floor) * 0.002;
        }
        let threshold = (self.noise_floor * 3.0).max(0.010);

        if rms > threshold {
            self.voiced_run += 1;
            // ~80 ms de voz contínua = começou a falar de verdade (e não foi
            // só um clique ou batida no teclado).
            if self.voiced_run >= 8 {
                self.heard_speech = true;
            }
            self.last_voice = Instant::now();
        } else {
            self.voiced_run = 0;
        }

        (self.heard_speech && self.last_voice.elapsed() >= VAD_SILENCE)
            || (!self.heard_speech && started.elapsed() >= VAD_NO_SPEECH_TIMEOUT)
    }
}

/// Encerra a gravação atual: derruba o stream, drena o que sobrou no canal
/// e emite o áudio completo como evento.
fn finish(
    stream: &mut Option<cpal::Stream>,
    data_rx: &mut Option<Receiver<Vec<f32>>>,
    samples: &mut Vec<f32>,
    sample_rate: u32,
    channels: u16,
    event_tx: &Sender<RecorderEvent>,
) {
    drop(stream.take()); // o callback (e o lado tx do canal) morre junto
    std::thread::sleep(Duration::from_millis(30));
    if let Some(rx) = data_rx.take() {
        for chunk in rx.try_iter() {
            samples.extend(chunk);
        }
    }
    let audio = RawAudio {
        samples: std::mem::take(samples),
        sample_rate,
        channels,
    };
    let _ = event_tx.send(RecorderEvent::Finished(Ok(audio)));
}

fn run(cmd_rx: Receiver<Command>, event_tx: Sender<RecorderEvent>, level_tx: Sender<f32>) {
    use cpal::traits::StreamTrait;

    let mut stream: Option<cpal::Stream> = None;
    let mut data_rx: Option<Receiver<Vec<f32>>> = None;
    let mut samples: Vec<f32> = Vec::new();
    let mut sample_rate = 0u32;
    let mut channels = 0u16;
    let mut vad = Vad::new();
    let mut started = Instant::now();

    loop {
        if let Some(rx) = data_rx.clone() {
            // Gravando: escuta comandos E dados ao mesmo tempo.
            crossbeam_channel::select! {
                recv(cmd_rx) -> cmd => match cmd {
                    Ok(Command::Stop) => {
                        finish(&mut stream, &mut data_rx, &mut samples, sample_rate, channels, &event_tx);
                    }
                    Ok(Command::SetVad(on)) => vad.enabled = on,
                    Ok(Command::Start) => {} // já gravando — ignora
                    Err(_) => return,        // app encerrou
                },
                recv(rx) -> chunk => if let Ok(chunk) = chunk {
                    let rms = (chunk.iter().map(|s| s * s).sum::<f32>()
                        / chunk.len().max(1) as f32)
                        .sqrt();
                    let _ = level_tx.try_send(rms);
                    samples.extend(chunk);
                    if vad.should_stop(rms, started) || started.elapsed() >= MAX_RECORDING {
                        tracing::info!("VAD encerrou a gravação");
                        finish(&mut stream, &mut data_rx, &mut samples, sample_rate, channels, &event_tx);
                    }
                },
            }
        } else {
            // Parado: só espera comando.
            match cmd_rx.recv() {
                Ok(Command::Start) => {
                    let (tx, rx) = unbounded();
                    match audio::open_input_stream(tx) {
                        Ok((s, rate, ch)) => {
                            if let Err(e) = s.play() {
                                let _ = event_tx
                                    .send(RecorderEvent::Finished(Err(IsperError::Audio(e.to_string()))));
                                continue;
                            }
                            sample_rate = rate;
                            channels = ch;
                            samples.clear();
                            vad = Vad::new();
                            started = Instant::now();
                            stream = Some(s);
                            data_rx = Some(rx);
                        }
                        Err(e) => {
                            let _ = event_tx.send(RecorderEvent::Finished(Err(e)));
                        }
                    }
                }
                Ok(Command::SetVad(on)) => vad.enabled = on,
                Ok(Command::Stop) => {} // não estava gravando — ignora
                Err(_) => return,
            }
        }
    }
}
