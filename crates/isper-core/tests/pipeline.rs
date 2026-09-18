//! Testes de integração e regressão do pipeline de transcrição.
//!
//! Divididos em dois grupos:
//!
//! - **sem modelo**: montam o pipeline com um áudio sintético e conferem as
//!   fronteiras (VAD, janelas, alinhamento, reconstrução das falas). Rodam em
//!   qualquer máquina, no CI inclusive;
//! - **com modelo** (`#[ignore]`): precisam do Whisper e do Silero instalados.
//!   Rode com
//!   `cargo test --release -p isper-core --test pipeline -- --ignored`.
//!
//! O corpus grande de regressão (uma reunião inteira, com verdade de
//! referência) vive fora daqui, no `isper-cli bench` — áudio de minutos não
//! pertence a um teste de unidade.

use isper_core::align::{self, AlignOptions, SpeakerTurn};
use isper_core::chunk::{self, ChunkOptions, CutReason};
use isper_core::context::MeetingContext;
use isper_core::engine::Word;
use isper_core::metrics::{self, Normalization, PipelineReport};
use isper_core::vad::{self, SpeechRegion, WindowOptions};
use isper_core::{WHISPER_SAMPLE_RATE, WhisperEngine};

/// Tom de `secs` segundos com a amplitude pedida — serve de "fala" sintética.
fn tom(secs: f32, amp: f32, hz: f32) -> Vec<f32> {
    let n = (WHISPER_SAMPLE_RATE as f32 * secs) as usize;
    (0..n)
        .map(|i| (i as f32 * hz * std::f32::consts::TAU / WHISPER_SAMPLE_RATE as f32).sin() * amp)
        .collect()
}

fn silencio(secs: f32) -> Vec<f32> {
    vec![0.0; (WHISPER_SAMPLE_RATE as f32 * secs) as usize]
}

// ------------------------------------------------------- sem modelo

#[test]
fn do_vad_a_fala_o_caminho_inteiro_se_sustenta() {
    // Três trechos de fala separados por silêncio de 3 s, dois falantes.
    let regions = [
        SpeechRegion {
            start_secs: 0.0,
            end_secs: 8.0,
        },
        SpeechRegion {
            start_secs: 11.0,
            end_secs: 19.0,
        },
        SpeechRegion {
            start_secs: 22.0,
            end_secs: 30.0,
        },
    ];
    let windows = vad::plan_windows(&regions, 31.0, &WindowOptions::default());
    assert_eq!(
        windows.len(),
        3,
        "silêncio de 3 s fecha janela: {windows:?}"
    );
    // Toda fronteira de janela cai fora de uma região de fala — é a garantia
    // que o corte por energia não dava.
    for w in &windows {
        for r in &regions {
            let dentro = |t: f32| t > r.start_secs + 0.3 && t < r.end_secs - 0.3;
            assert!(!dentro(w.start_secs), "janela começa dentro da fala: {w:?}");
            assert!(!dentro(w.end_secs), "janela termina dentro da fala: {w:?}");
        }
    }

    // Palavras dessas janelas, dois falantes alternando.
    let palavras: Vec<Word> = (0..30)
        .map(|i| Word {
            text: format!("p{i}"),
            start_secs: i as f32,
            end_secs: i as f32 + 0.8,
            prob: 0.9,
        })
        .collect();
    let turnos = [
        SpeakerTurn {
            start_secs: 0.0,
            end_secs: 15.0,
            speaker: 0,
        },
        SpeakerTurn {
            start_secs: 15.0,
            end_secs: 31.0,
            speaker: 1,
        },
    ];
    let opts = AlignOptions::default();
    let marcadas = align::tag_words(&palavras, &turnos, &opts);
    assert!(marcadas.iter().all(|w| w.speaker.is_some()));
    let falas = align::build_utterances(&marcadas, &opts);
    let m = align::speaker_metrics(&falas);
    assert_eq!(m.speakers, 2);
    assert_eq!(m.unassigned, 0);
    // Uma troca de falante, e a fronteira cai onde a diarização disse.
    assert_eq!(m.switches, 1, "{falas:#?}");
    let troca = falas
        .windows(2)
        .find(|p| p[0].speaker != p[1].speaker)
        .expect("há uma troca");
    assert!(
        (troca[1].start_secs - 15.0).abs() < 1.5,
        "troca em {:.1}s, esperada perto de 15 s",
        troca[1].start_secs
    );
}

#[test]
fn nenhuma_palavra_se_perde_entre_as_janelas() {
    // Fala contínua de 100 s sem pausa: o planejador precisa partir, e as
    // janelas têm de cobrir tudo, com sobreposição em vez de buraco.
    let regions = [SpeechRegion {
        start_secs: 0.0,
        end_secs: 100.0,
    }];
    let w = vad::plan_windows(&regions, 100.0, &WindowOptions::default());
    assert!(w.len() > 1);
    for par in w.windows(2) {
        assert!(
            par[1].start_secs <= par[0].end_secs,
            "buraco entre {:?} e {:?}",
            par[0],
            par[1]
        );
    }
    assert!(w[0].start_secs <= 0.0);
    assert!(w.last().expect("janelas").end_secs >= 99.9);
}

#[test]
fn o_corte_ao_vivo_nao_parte_palavra_quando_ha_silencio() {
    // Reproduz o caso "ma|nual": fala corrida até o alvo de 20 s. O corte
    // antigo saía ali mesmo; o novo espera o silêncio.
    let mut buf = tom(22.0, 0.35, 220.0);
    assert_eq!(
        chunk::plan_cut(&buf, WHISPER_SAMPLE_RATE, 1, &ChunkOptions::default()),
        None,
        "sem silêncio, não se corta"
    );
    buf.extend(silencio(0.6));
    buf.extend(tom(1.0, 0.35, 220.0));
    let cut = chunk::plan_cut(&buf, WHISPER_SAMPLE_RATE, 1, &ChunkOptions::default())
        .expect("agora há onde cortar");
    assert_eq!(cut.reason, CutReason::Silence);
    let at = cut.at as f32 / WHISPER_SAMPLE_RATE as f32;
    assert!((22.0..=22.6).contains(&at), "cortou em {at:.2}s");
}

#[test]
fn o_glossario_vira_prompt_e_correcao_sem_apagar_o_bruto() {
    let ctx = MeetingContext {
        participants: vec!["Paulo Rocha".into()],
        client: Some("Tommasi".into()),
        vocabulary: vec!["Optsolv".into()],
        acronyms: vec!["MRP".into()],
        ..Default::default()
    };
    let prompt = ctx.initial_prompt().expect("prompt");
    assert!(
        prompt.contains("Tommasi") && prompt.contains("MRP"),
        "{prompt}"
    );

    let bruto = "o paulo rocha da optsolv falou do mrp";
    let corrigido = isper_core::text::apply_dictionary(bruto, &ctx.terms());
    assert!(corrigido.contains("Paulo Rocha"), "{corrigido}");
    assert!(corrigido.contains("Optsolv"), "{corrigido}");
    assert!(corrigido.contains("MRP"), "{corrigido}");
    // E o bruto continua o que era: a normalização é uma segunda via.
    assert_eq!(bruto, "o paulo rocha da optsolv falou do mrp");
}

#[test]
fn as_taxas_de_erro_enxergam_os_erros_do_relato_do_usuario() {
    let n = Normalization::default();
    // Os dois erros que motivaram este trabalho.
    let w = metrics::wer(
        "precisamos revisar o manual e o fluxo de caixa",
        "precisamos revisar o anual e o flores de caixa",
        &n,
    );
    assert_eq!(w.substitutions, 2, "{w:?}");
    assert_eq!(w.reference_len, 9);
    assert!((w.rate - 2.0 / 9.0).abs() < 0.001, "{w:?}");
    // O CER separa "manual→anual" (1 caractere) de "fluxo→flores" (3).
    let c = metrics::cer(
        "precisamos revisar o manual e o fluxo de caixa",
        "precisamos revisar o anual e o flores de caixa",
        &n,
    );
    assert!(c.rate < w.rate);
    assert!(c.errors() >= 4, "{c:?}");
}

// ------------------------------------------------------- com modelo

/// Caminho do modelo Whisper instalado, se houver.
fn modelo() -> Option<std::path::PathBuf> {
    isper_models::resolve_whisper_model(None, false, &[])
}

#[test]
#[ignore = "precisa do modelo Whisper instalado"]
fn transcreve_com_beam_search_e_devolve_palavras_com_horario() {
    let Some(modelo) = modelo() else {
        eprintln!("sem modelo instalado — pulando");
        return;
    };
    let raw = isper_core::audio::load_wav(std::path::Path::new("../../fixtures/fala-16k.wav"))
        .expect("fixture fala-16k.wav");
    let samples = raw.into_whisper_input().expect("resample");
    let engine = WhisperEngine::new_with(&modelo, &isper_core::EngineOptions { dtw: true })
        .expect("carrega o modelo");

    let decode = isper_core::DecodeConfig::meeting_final();
    let t = engine
        .transcribe_with(
            &samples,
            isper_core::engine::TranscribeRequest {
                lang: "pt",
                decode: &decode,
                prompt: None,
            },
        )
        .expect("transcreve");

    assert!(!t.text.trim().is_empty(), "veio texto vazio");
    let palavras: Vec<&Word> = t.segments.iter().flat_map(|s| &s.words).collect();
    assert!(
        palavras.len() >= 5,
        "o perfil final tem que devolver palavras com horário, veio {}",
        palavras.len()
    );
    // Os horários das palavras são crescentes e cabem no áudio.
    let dur = samples.len() as f32 / WHISPER_SAMPLE_RATE as f32;
    let mut anterior = -1.0f32;
    for p in &palavras {
        assert!(
            p.start_secs >= anterior - 0.5,
            "palavras fora de ordem: {p:?}"
        );
        assert!(p.end_secs <= dur + 1.0, "palavra além do áudio: {p:?}");
        anterior = p.start_secs;
    }
}

#[test]
#[ignore = "precisa dos modelos Whisper e Silero instalados"]
fn o_passe_final_transcreve_um_wav_inteiro_e_relata_o_que_fez() {
    let Some(modelo) = modelo() else {
        eprintln!("sem modelo instalado — pulando");
        return;
    };
    let Ok(vad_model) = isper_models::vad_path() else {
        return;
    };
    if !vad_model.exists() {
        eprintln!("sem modelo de VAD — pulando");
        return;
    }
    let raw =
        isper_core::audio::load_wav(std::path::Path::new("../../fixtures/duas-vozes-16k.wav"))
            .expect("fixture duas-vozes-16k.wav");
    let samples = raw.into_whisper_input().expect("resample");
    let engine = WhisperEngine::new_with(&modelo, &isper_core::EngineOptions { dtw: true })
        .expect("carrega o modelo");

    let cfg = isper_core::pipeline::FinalConfig::meeting_final(vad_model);
    let out = isper_core::pipeline::run(
        &engine,
        &samples,
        &cfg,
        &MeetingContext::default(),
        None,
        None,
    )
    .expect("passe final");

    assert!(!out.raw_text.trim().is_empty());
    assert!(!out.utterances.is_empty());
    let r = &out.report;
    assert!(r.vad.regions > 0, "o VAD não achou fala nenhuma");
    assert!(r.vad.windows > 0);
    assert!(r.asr.words > 0, "o passe final precisa de palavras");
    assert_eq!(r.asr.failed_windows, 0);
    assert!(r.timings.total_secs > 0.0);
    // Sem diarização, ninguém ganha falante — e isso aparece na métrica.
    assert!(r.diarization.is_none());
    assert_eq!(r.speakers.unassigned, out.utterances.len());
    // O relatório vai e volta do JSON: é assim que o benchmark o guarda.
    let volta: PipelineReport = serde_json::from_str(&r.to_json()).expect("relatório serializável");
    assert_eq!(volta.config, r.config);
}

#[test]
#[ignore = "precisa dos modelos Whisper e Silero instalados"]
fn o_passe_final_ganha_do_fatiamento_antigo_no_mesmo_audio() {
    let Some(modelo) = modelo() else { return };
    let Ok(vad_model) = isper_models::vad_path() else {
        return;
    };
    if !vad_model.exists() {
        return;
    }
    let raw =
        isper_core::audio::load_wav(std::path::Path::new("../../fixtures/duas-vozes-16k.wav"))
            .expect("fixture");
    let samples = raw.into_whisper_input().expect("resample");
    let engine =
        WhisperEngine::new_with(&modelo, &isper_core::EngineOptions { dtw: true }).expect("modelo");
    let ctx = MeetingContext::default();

    let base = isper_core::pipeline::run(
        &engine,
        &samples,
        &isper_core::pipeline::FinalConfig::baseline(),
        &ctx,
        None,
        None,
    )
    .expect("baseline");
    let novo = isper_core::pipeline::run(
        &engine,
        &samples,
        &isper_core::pipeline::FinalConfig::meeting_final(vad_model),
        &ctx,
        None,
        None,
    )
    .expect("final");

    // O que se garante aqui é estrutural, não a taxa de erro (que depende do
    // modelo instalado e tem corpus próprio no `bench`): o passe final
    // enxerga o silêncio, produz palavras com horário e usa contexto.
    assert_eq!(
        base.report.asr.words, 0,
        "o perfil ao vivo não pede palavras"
    );
    assert!(novo.report.asr.words > 0);
    assert!(novo.report.vad.discarded_silence_secs > 0.0);
    assert_eq!(base.report.vad.regions, 0);
    assert!(
        novo.report.params.context_chars > 0,
        "contexto entre janelas ligado"
    );
    // A fixture tem 29 s e cabe numa janela só — só há contexto a herdar
    // quando há mais de uma. Onde houver, tem que ter sido usado.
    if novo.report.vad.windows > 1 {
        assert!(novo.report.asr.windows_with_context > 0);
    }
    // E o bruto continua guardado ao lado do normalizado, sempre.
    assert!(!novo.raw_text.is_empty());
    assert!(!novo.raw_segments.is_empty());
}
