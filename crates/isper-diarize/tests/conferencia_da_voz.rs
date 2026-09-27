//! A conferência com a voz inteira com os modelos de verdade, nas fixtures.
//!
//! `cargo test --release -p isper-diarize -- --ignored` — sem os modelos de
//! diarização instalados (`isper-cli models download-diarize`), os testes
//! avisam e passam sem rodar.

use std::path::Path;

use isper_diarize::DiarizeOptions;

fn fixture(name: &str) -> Vec<f32> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    let mut reader = hound::WavReader::open(&path).expect("fixture");
    let spec = reader.spec();
    assert_eq!((spec.sample_rate, spec.channels), (16_000, 1), "{name}");
    reader
        .samples::<i16>()
        .map(|s| f32::from(s.expect("amostra")) / 32_768.0)
        .collect()
}

fn models() -> bool {
    let ok = isper_diarize::models_installed();
    if !ok {
        eprintln!("sem os modelos de diarização — pulando");
    }
    ok
}

#[test]
#[ignore = "precisa dos modelos de diarização instalados"]
fn a_mesma_voz_partida_pelo_agrupamento_volta_a_ser_uma() {
    if !models() {
        return;
    }
    // Sem a absorção, o agrupamento parte a primeira voz da fixture em dois
    // grupos com semelhança de 0,90 entre eles: a conferência junta de volta.
    let opts = DiarizeOptions {
        min_speaker_secs: 0.0,
        min_speaker_share: 0.0,
        min_speaker_turns: 0,
        ..Default::default()
    };
    let out =
        isper_diarize::diarize_with(&fixture("duas-vozes-16k.wav"), &opts).expect("diarização");
    let m = &out.metrics;
    assert_eq!(
        m.merged_same_voice, 1,
        "a premissa mudou: o agrupamento já não parte a voz? {m:?}"
    );
    assert_eq!(m.speakers, 2, "{m:?}");
}

#[test]
#[ignore = "precisa dos modelos de diarização instalados"]
fn vozes_parecidas_continuam_separadas() {
    if !models() {
        return;
    }
    // As três "pessoas" da reunião sintética são a mesma voz a ±8 % de
    // velocidade: o pior caso de vozes diferentes. Nenhuma pode sumir.
    let out = isper_diarize::diarize_with(
        &fixture("reuniao-sintetica-16k.wav"),
        &DiarizeOptions::default(),
    )
    .expect("diarização");
    let m = &out.metrics;
    assert_eq!(m.merged_same_voice, 0, "{:?}", m.voice_similarities);
    assert_eq!(m.speakers, 3, "{m:?}");
}
