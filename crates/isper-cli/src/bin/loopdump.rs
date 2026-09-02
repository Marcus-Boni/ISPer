//! Depuração do loopback: captura N segundos do áudio do sistema, mostra a
//! taxa de entrega a cada segundo e salva um WAV para inspeção.
//! Uso: cargo run --release -p isper-cli --bin loopdump -- 15 [system|teams|process:x.exe]

use std::time::{Duration, Instant};

use isper_core::loopback::LoopbackSource;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_target(false).compact().init();
    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(15);
    let source = LoopbackSource::parse(&std::env::args().nth(2).unwrap_or_default());

    let (stop_tx, stop_rx) = crossbeam_channel::unbounded();
    let (data_tx, data_rx) = crossbeam_channel::unbounded();
    let (ready_tx, ready_rx) = crossbeam_channel::unbounded();
    std::thread::spawn(move || isper_core::loopback::run(source, stop_rx, data_tx, ready_tx));

    let ready = ready_rx.recv_timeout(Duration::from_secs(8))??;
    let (rate, ch) = (ready.sample_rate, ready.channels);
    if let Some(w) = &ready.warning {
        println!("aviso: {w}");
    }
    println!("loopback aberto: {rate} Hz, {ch} canais — gravando {secs}s...");

    let started = Instant::now();
    let mut all: Vec<f32> = Vec::new();
    let mut last_report = 0u64;
    while started.elapsed().as_secs() < secs {
        if let Ok(chunk) = data_rx.recv_timeout(Duration::from_millis(200)) {
            all.extend(chunk);
        }
        let s = started.elapsed().as_secs();
        if s > last_report {
            last_report = s;
            let delivered = all.len() as f32 / ch as f32 / rate as f32;
            println!("t={s:>2}s | entregue: {delivered:>5.1}s");
        }
    }
    let _ = stop_tx.send(());

    let delivered = all.len() as f32 / ch as f32 / rate as f32;
    println!("total entregue: {delivered:.1}s em {secs}s de parede");

    let spec = hound::WavSpec {
        channels: ch,
        sample_rate: rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create("loopdump.wav", spec)?;
    for s in &all {
        writer.write_sample(*s)?;
    }
    writer.finalize()?;
    println!("salvo: loopdump.wav");
    Ok(())
}
