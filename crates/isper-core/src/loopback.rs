//! Captura de loopback — o áudio que está saindo na caixa de som — via
//! crate `wasapi`, com keepalive integrado e auto-recuperação.
//!
//! O que aprendemos na prática nesta máquina (endpoint de áudio USB):
//!
//! - Loopback só entrega dados enquanto o endpoint está ATIVO; quando nada
//!   toca, o dispositivo suspende e o stream estagna — às vezes sem voltar.
//! - Streams event-driven (cpal, inclusive um keepalive via cpal) morrem
//!   junto: os eventos simplesmente param de chegar.
//!
//! A defesa, toda em UMA thread com polling (nós controlamos a cadência):
//!
//! 1. **Keepalive integrado**: um cliente de RENDER no mesmo endpoint recebe
//!    silêncio a cada ciclo do loop — o dispositivo nunca fica ocioso;
//! 2. **Drenagem completa**: `GetBuffer` devolve UM pacote (~10 ms) por
//!    chamada, então drenamos até esvaziar a cada ciclo;
//! 3. **Watchdog**: com o keepalive, um loopback saudável entrega
//!    continuamente (silêncio incluído) — >2 s sem NENHUM pacote significa
//!    que estagnou: fechamos e reabrimos tudo, e a captura segue.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};

use crate::{IsperError, Result};

/// Formato pedido ao WASAPI (com `autoconvert`, o mixer converte p/ nós).
const RATE: usize = 48_000;
const CH: usize = 2;
const BYTES_PER_FRAME: usize = 4 * CH; // f32 intercalado
/// Buffer dos clientes: 500 ms de folga (unidades de 100 ns).
const BUFFER_HNS: i64 = 5_000_000;
/// Sem nenhum byte novo por este tempo = stream estagnado → reabre.
/// (Com o keepalive, um loopback saudável entrega ininterruptamente.)
const STALL_TIMEOUT: Duration = Duration::from_millis(1000);

/// Roda o loop de captura até chegar QUALQUER coisa em `stop_rx` (ou o canal
/// fechar). Deve rodar numa thread própria (inicializa COM nela). Amostras
/// f32 intercaladas (48 kHz, 2 canais) saem por `tx`; o resultado da
/// abertura sai por `ready_tx` como `(sample_rate, channels)`.
pub fn run(stop_rx: Receiver<()>, tx: Sender<Vec<f32>>, ready_tx: Sender<Result<(u32, u16)>>) {
    if let Err(e) = wasapi::initialize_mta().ok() {
        let _ = ready_tx.send(Err(IsperError::Audio(format!("COM: {e}"))));
        return;
    }
    match open() {
        Ok(session) => {
            let _ = ready_tx.send(Ok((RATE as u32, CH as u16)));
            pump(session, stop_rx, tx);
        }
        Err(e) => {
            let _ = ready_tx.send(Err(e));
        }
    }
}

struct Session {
    capture_client: wasapi::AudioClient,
    capture: wasapi::AudioCaptureClient,
    event: wasapi::Handle,
    render_client: wasapi::AudioClient,
    render: wasapi::AudioRenderClient,
    /// Zeros reutilizáveis para alimentar o keepalive (1 s de silêncio).
    silence: Vec<u8>,
}

fn wa<T>(r: std::result::Result<T, wasapi::WasapiError>, what: &str) -> Result<T> {
    r.map_err(|e| IsperError::Audio(format!("loopback ({what}): {e}")))
}

fn open() -> Result<Session> {
    let enumerator = wa(wasapi::DeviceEnumerator::new(), "enumerator")?;
    let device = wa(
        enumerator.get_default_device(&wasapi::Direction::Render),
        "dispositivo de saída",
    )?;

    let format = wasapi::WaveFormat::new(32, 32, &wasapi::SampleType::Float, RATE, CH, None);

    // Captura em loopback: dispositivo de RENDER + direção CAPTURE (shared).
    let mut capture_client = wa(device.get_iaudioclient(), "audio client (captura)")?;
    let cap_mode = wasapi::StreamMode::EventsShared {
        autoconvert: true,
        buffer_duration_hns: BUFFER_HNS,
    };
    wa(
        capture_client.initialize_client(&format, &wasapi::Direction::Capture, &cap_mode),
        "initialize captura",
    )?;
    let event = wa(capture_client.set_get_eventhandle(), "event handle")?;
    let capture = wa(capture_client.get_audiocaptureclient(), "capture client")?;
    wa(capture_client.start_stream(), "start captura")?;

    // Keepalive: cliente de RENDER no mesmo endpoint, em POLLING — escrevemos
    // silêncio a cada ciclo e o dispositivo nunca suspende.
    let mut render_client = wa(device.get_iaudioclient(), "audio client (render)")?;
    let ren_mode = wasapi::StreamMode::PollingShared {
        autoconvert: true,
        buffer_duration_hns: BUFFER_HNS,
    };
    wa(
        render_client.initialize_client(&format, &wasapi::Direction::Render, &ren_mode),
        "initialize render",
    )?;
    let render = wa(render_client.get_audiorenderclient(), "render client")?;
    wa(render_client.start_stream(), "start render")?;

    let name = device.get_friendlyname().unwrap_or_default();
    tracing::info!(device = %name, sample_rate = RATE, channels = CH, "capturando loopback (com keepalive)");
    Ok(Session {
        capture_client,
        capture,
        event,
        render_client,
        render,
        silence: vec![0u8; RATE * BYTES_PER_FRAME],
    })
}

fn pump(mut session: Session, stop_rx: Receiver<()>, tx: Sender<Vec<f32>>) {
    let mut bytes: VecDeque<u8> = VecDeque::with_capacity(RATE * BYTES_PER_FRAME);
    let mut last_packet = Instant::now();

    loop {
        // Encerra tanto no sinal de stop quanto se o outro lado desligar.
        match stop_rx.try_recv() {
            Err(crossbeam_channel::TryRecvError::Empty) => {}
            _ => break,
        }

        // Alimenta o keepalive: preenche o espaço livre com silêncio.
        if let Ok(space) = session.render_client.get_available_space_in_frames() {
            let frames = (space as usize).min(session.silence.len() / BYTES_PER_FRAME);
            if frames > 0 {
                let _ = session.render.write_to_device(
                    frames,
                    &session.silence[..frames * BYTES_PER_FRAME],
                    None,
                );
            }
        }

        // Espera o evento, mas timeout NÃO é erro: drenamos de qualquer jeito.
        let _ = session.event.wait_for_event(100);

        // "Sucesso" de leitura não basta: já vimos o cliente entrar num estado
        // em que GetNextPacketSize acusa dados mas GetBuffer devolve 0 frames
        // para sempre. Só bytes NOVOS contam como sinal de vida.
        let before_len = bytes.len();
        let mut reads = 0;
        loop {
            if reads > 400 {
                break; // trava de segurança contra loop infinito
            }
            match session.capture.get_next_packet_size() {
                Ok(Some(frames)) if frames > 0 => {
                    if let Err(e) = session.capture.read_from_device_to_deque(&mut bytes) {
                        tracing::warn!("loopback: leitura falhou: {e}");
                        break;
                    }
                    if bytes.len() == before_len && reads > 4 {
                        break; // acusa pacotes mas não entrega nada — desiste do ciclo
                    }
                    reads += 1;
                }
                Ok(_) => break,
                Err(e) => {
                    tracing::warn!("loopback: packet size falhou: {e}");
                    break;
                }
            }
        }

        if bytes.len() > before_len {
            last_packet = Instant::now();
            let take = bytes.len() - (bytes.len() % 4);
            if take > 0 {
                let raw: Vec<u8> = bytes.drain(..take).collect();
                let floats: Vec<f32> = raw
                    .chunks_exact(4)
                    .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .collect();
                if tx.send(floats).is_err() {
                    break;
                }
            }
        } else if last_packet.elapsed() > STALL_TIMEOUT {
            tracing::warn!("loopback estagnou — reabrindo o cliente");
            let _ = session.capture_client.stop_stream();
            let _ = session.render_client.stop_stream();
            match open() {
                Ok(s) => {
                    session = s;
                    last_packet = Instant::now();
                }
                Err(e) => {
                    tracing::warn!("loopback: falha ao reabrir ({e}) — nova tentativa em 500 ms");
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
        }
    }
    let _ = session.capture_client.stop_stream();
    let _ = session.render_client.stop_stream();
}
