//! Gravador controlável por comandos — o coração do push-to-talk (Fase 2).
//!
//! O `cpal::Stream` não é `Send`: ele precisa viver inteiro numa única
//! thread. Por isso este módulo sobe uma *thread de áudio* dedicada, dona do
//! stream, e conversa com o resto do app só por canais:
//!
//! - `start()` / `stop()` chegam por um canal de comandos;
//! - o áudio gravado volta por um canal de resultados;
//! - o nível do microfone (RMS) sai continuamente por um canal de níveis,
//!   que a UI usa para desenhar o waveform.

use crossbeam_channel::{bounded, unbounded, Receiver, Sender};

use crate::{audio, IsperError, RawAudio, Result};

enum Command {
    Start,
    Stop,
}

/// Alça para a thread de áudio. `Send + Sync` — pode viver no estado do Tauri.
pub struct AudioHandle {
    cmd_tx: Sender<Command>,
    result_rx: Receiver<Result<RawAudio>>,
    level_rx: Receiver<f32>,
}

impl AudioHandle {
    /// Começa a gravar (não bloqueia). Se já estiver gravando, é ignorado.
    pub fn start(&self) {
        let _ = self.cmd_tx.send(Command::Start);
    }

    /// Para de gravar e devolve tudo que foi capturado desde o `start`.
    pub fn stop(&self) -> Result<RawAudio> {
        self.cmd_tx
            .send(Command::Stop)
            .map_err(|_| IsperError::Audio("thread de áudio encerrou".into()))?;
        self.result_rx
            .recv()
            .map_err(|_| IsperError::Audio("thread de áudio encerrou".into()))?
    }

    /// Canal com o nível RMS do microfone (~100 leituras/s durante a gravação).
    pub fn levels(&self) -> Receiver<f32> {
        self.level_rx.clone()
    }
}

/// Sobe a thread de áudio e devolve a alça de controle.
pub fn spawn() -> AudioHandle {
    let (cmd_tx, cmd_rx) = unbounded();
    let (result_tx, result_rx) = unbounded();
    // Bounded + try_send: se a UI não consumir os níveis a tempo, descartamos
    // leituras antigas em vez de acumular memória.
    let (level_tx, level_rx) = bounded(64);
    std::thread::spawn(move || run(cmd_rx, result_tx, level_tx));
    AudioHandle {
        cmd_tx,
        result_rx,
        level_rx,
    }
}

fn run(cmd_rx: Receiver<Command>, result_tx: Sender<Result<RawAudio>>, level_tx: Sender<f32>) {
    use cpal::traits::StreamTrait;

    let mut stream: Option<cpal::Stream> = None;
    let mut data_rx: Option<Receiver<Vec<f32>>> = None;
    let mut samples: Vec<f32> = Vec::new();
    let mut sample_rate = 0u32;
    let mut channels = 0u16;

    loop {
        if let Some(rx) = data_rx.clone() {
            // Gravando: escuta comandos E dados ao mesmo tempo.
            crossbeam_channel::select! {
                recv(cmd_rx) -> cmd => match cmd {
                    Ok(Command::Stop) => {
                        drop(stream.take()); // encerra a captura (callback morre junto)
                        std::thread::sleep(std::time::Duration::from_millis(30));
                        for chunk in rx.try_iter() {
                            samples.extend(chunk);
                        }
                        data_rx = None;
                        let audio = RawAudio {
                            samples: std::mem::take(&mut samples),
                            sample_rate,
                            channels,
                        };
                        let _ = result_tx.send(Ok(audio));
                    }
                    Ok(Command::Start) => {} // já gravando — ignora
                    Err(_) => return,        // app encerrou
                },
                recv(rx) -> chunk => if let Ok(chunk) = chunk {
                    let rms = (chunk.iter().map(|s| s * s).sum::<f32>()
                        / chunk.len().max(1) as f32)
                        .sqrt();
                    let _ = level_tx.try_send(rms);
                    samples.extend(chunk);
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
                                let _ = result_tx.send(Err(IsperError::Audio(e.to_string())));
                                continue;
                            }
                            sample_rate = rate;
                            channels = ch;
                            samples.clear();
                            stream = Some(s);
                            data_rx = Some(rx);
                        }
                        Err(e) => {
                            let _ = result_tx.send(Err(e));
                        }
                    }
                }
                Ok(Command::Stop) => {
                    let _ = result_tx.send(Err(IsperError::Audio("não estava gravando".into())));
                }
                Err(_) => return,
            }
        }
    }
}
