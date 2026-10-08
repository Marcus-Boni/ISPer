//! O loopback de verdade, num dispositivo de saída de verdade.
//!
//! Prova que o `wasapi` abre a captura do que sai na caixa de som e entrega
//! áudio mesmo em silêncio (o keepalive do ADR 0004 toca silêncio para o
//! WASAPI não parar de mandar pacotes). Não toca som nenhum.
//!
//! Precisa de uma saída de áudio, então fica fora do CI. À mão, depois de
//! trocar a versão do `wasapi`:
//!
//! ```text
//! cargo test --release --no-default-features -p isper-core --test loopback_device -- --ignored
//! ```
#![cfg(windows)]

use std::time::{Duration, Instant};

use isper_core::loopback::{self, LoopbackSource};

#[test]
#[ignore = "precisa de um dispositivo de saída de áudio"]
fn o_loopback_abre_e_entrega_audio_mesmo_em_silencio() {
    let (stop_tx, stop_rx) = crossbeam_channel::bounded(1);
    let (tx, rx) = crossbeam_channel::unbounded();
    let (ready_tx, ready_rx) = crossbeam_channel::bounded(1);
    let captura =
        std::thread::spawn(move || loopback::run(LoopbackSource::System, stop_rx, tx, ready_tx));

    let ready = ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("o loopback respondeu em 5 s")
        .expect("o loopback abriu");
    assert_eq!(ready.sample_rate, 48_000);
    assert_eq!(ready.channels, 2);

    let mut amostras = 0usize;
    let fim = Instant::now() + Duration::from_secs(2);
    while Instant::now() < fim {
        if let Ok(bloco) = rx.recv_timeout(Duration::from_millis(200)) {
            amostras += bloco.len();
        }
    }
    stop_tx
        .send(())
        .expect("a captura ainda escuta o pedido de parar");
    captura
        .join()
        .expect("a thread da captura terminou sem pânico");

    // 2 s a 48 kHz estéreo são ~192 mil amostras; metade já prova o fluxo.
    assert!(
        amostras > 96_000,
        "chegaram só {amostras} amostras em 2 s de captura"
    );
}

#[test]
#[ignore = "precisa do serviço de áudio do Windows"]
fn a_deteccao_de_chamada_le_as_sessoes_de_audio() {
    // O Explorer sempre roda, então o `probe` passa da lista de processos e
    // enumera de verdade os dispositivos e as sessões de áudio (com o Teams
    // fechado, ele voltaria antes de tocar no `wasapi`). A resposta em si não
    // importa; importa ler as sessões sem erro.
    isper_core::calls::probe(&["explorer.exe"]).expect("as sessões de áudio foram lidas");
}
