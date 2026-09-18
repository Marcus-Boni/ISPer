//! Passe final da reunião: a transcrição oficial, feita depois que a reunião
//! acaba.
//!
//! Durante a reunião o ISPer transcreve em blocos para dar retorno na hora —
//! e paga por isso: decodificação rápida, sem contexto entre blocos, sem
//! falante. Quando a gravação encerra, o áudio inteiro dos dois canais está
//! em disco ([`isper_core::meeting::ChannelAudio`]), e aí vale a pena refazer
//! tudo com calma:
//!
//! ```text
//! canal "Eu"            canal "Participantes"
//!      │                        │
//!      └──── VAD → ASR ─────────┘
//!            (beam search, contexto, palavras com timestamp)
//!                 │              │
//!                 │         diarização
//!                 │              │
//!                 │      falante por palavra
//!                 └──────┬───────┘
//!                  falas em ordem
//!                        │
//!               substitui a transcrição
//! ```
//!
//! O que sai daqui é o que a Biblioteca, o `.md`, a busca e o resumo por IA
//! passam a usar. Se qualquer etapa falhar, a transcrição ao vivo continua no
//! lugar: o passe final só substitui quando termina inteiro.

use crate::prelude::*;
use isper_core::align::SpeakerTurn;
use isper_core::context::MeetingContext;
use isper_core::meeting;
use isper_core::meeting::MeetingResult;
use isper_core::pipeline::{self, Diarizer, DiarizerOutput, FinalConfig, FinalTranscript};

/// Uma fala pronta para o banco: falante, início, fim, texto.
type Row = (String, f32, f32, String);

/// Liga o `isper-diarize` ao pipeline do core (que não conhece o sherpa).
struct SherpaDiarizer {
    opts: isper_diarize::DiarizeOptions,
}

impl Diarizer for SherpaDiarizer {
    fn diarize(&self, samples_16k: &[f32]) -> Result<DiarizerOutput, String> {
        let out =
            isper_diarize::diarize_with(samples_16k, &self.opts).map_err(|e| e.to_string())?;
        Ok(DiarizerOutput {
            turns: out
                .turns
                .iter()
                .map(|t| SpeakerTurn {
                    start_secs: t.start,
                    end_secs: t.end,
                    speaker: t.speaker,
                })
                .collect(),
            stats: isper_core::metrics::DiarizeStats {
                raw_clusters: out.metrics.raw_clusters,
                raw_turns: out.metrics.raw_turns,
                speakers: out.metrics.speakers,
                turns: out.metrics.turns,
                absorbed_clusters: out.metrics.absorbed_clusters,
                median_turn_secs: out.metrics.median_turn_secs,
                very_short_turns: out.metrics.very_short_turns,
                warnings: Vec::new(),
            },
            warnings: out.warnings,
        })
    }
}

/// Opções do passe final montadas a partir da configuração do app.
struct Options {
    lang: String,
    context: MeetingContext,
    speakers: Option<u32>,
    diarize_threshold: Option<f32>,
}

fn options(app: &AppHandle) -> Options {
    let cfg = app.state::<AppState>().config.lock_or_recover().clone();
    Options {
        lang: cfg.lang.clone(),
        context: MeetingContext::from_dictionary(&cfg.dictionary),
        speakers: (cfg.meeting_speakers > 0).then_some(cfg.meeting_speakers),
        diarize_threshold: (cfg.diarize_threshold > 0.0).then_some(cfg.diarize_threshold),
    }
}

/// Roda o passe final numa thread e, ao terminar, substitui a transcrição da
/// reunião no banco e regrava o Markdown. Avisa as janelas nas duas pontas.
pub(crate) fn run_in_background(app: AppHandle, meeting_id: i64, result: MeetingResult) {
    {
        let state = app.state::<AppState>();
        *state.diarizing.lock_or_recover() = Some(meeting_id);
    }
    notify_status(&app);
    let _ = app.emit("isper-state", json!({"state": "meeting-final"}));

    std::thread::spawn(move || {
        let started = Instant::now();
        match run(&app, meeting_id, &result) {
            Ok(Some(n)) => tracing::info!(
                meeting_id,
                falas = n,
                secs = started.elapsed().as_secs_f32(),
                "transcrição final publicada"
            ),
            Ok(None) => tracing::info!(meeting_id, "passe final desligado na configuração"),
            Err(e) => tracing::warn!(
                meeting_id,
                "passe final falhou ({e}) — a transcrição ao vivo foi mantida"
            ),
        }
        {
            let state = app.state::<AppState>();
            let mut d = state.diarizing.lock_or_recover();
            if *d == Some(meeting_id) {
                *d = None;
            }
        }
        notify_status(&app);
        let _ = app.emit("isper-state", json!({"state": "meeting-done"}));
    });
}

fn run(app: &AppHandle, meeting_id: i64, result: &MeetingResult) -> anyhow::Result<Option<usize>> {
    let opts = options(app);
    if !app.state::<AppState>().config.lock_or_recover().final_pass {
        return Ok(None);
    }
    let engine = app
        .state::<AppState>()
        .engine
        .lock_or_recover()
        .clone()
        .ok_or_else(|| anyhow::anyhow!("o modelo Whisper não está carregado"))?;

    // O modelo de VAD tem 0,9 MB e é baixado na primeira reunião.
    let vad_model = isper_models::vad_path()?;
    if !vad_model.exists() {
        tracing::info!(
            "baixando o modelo de VAD ({} MB)",
            isper_models::VAD_APPROX_MB
        );
        isper_models::download_vad(&mut |_, _| {})?;
    }

    let mut rows: Vec<Row> = Vec::new();

    // Canal dos participantes: com diarização, se os modelos existirem.
    let others = result.others_audio_f32()?;
    if !others.is_empty() {
        let diarizer = isper_diarize::models_installed().then(|| SherpaDiarizer {
            opts: isper_diarize::DiarizeOptions {
                num_speakers: opts.speakers,
                threshold: opts
                    .diarize_threshold
                    .unwrap_or(isper_diarize::DEFAULT_THRESHOLD),
                ..isper_diarize::DiarizeOptions::default()
            },
        });
        let cfg = FinalConfig {
            name: "participantes".into(),
            lang: opts.lang.clone(),
            ..FinalConfig::meeting_final(vad_model.clone())
        };
        let out = pipeline::run(
            &engine,
            &others,
            &cfg,
            &opts.context,
            diarizer.as_ref().map(|d| d as &dyn Diarizer),
            None,
        )?;
        log_report(meeting_id, "participantes", &out);
        // Agrupamento implausível: publicamos o texto, não os falantes. Um
        // rótulo genérico é menos errado que dezenas de pessoas inventadas.
        let confiavel = out
            .report
            .diarization
            .as_ref()
            .is_none_or(|d| d.warnings.is_empty());
        if !confiavel {
            tracing::warn!(
                meeting_id,
                "diarização implausível — as falas ficam como \"Participantes\""
            );
        }
        rows.extend(out.utterances.iter().map(|u| {
            let speaker = match u.speaker.filter(|_| confiavel) {
                Some(n) => meeting::Speaker::Participant(n + 1).label(),
                None => meeting::Speaker::Others.label(),
            };
            (speaker, u.start_secs, u.end_secs, u.text.clone())
        }));
    }

    // Canal do microfone: quem falou já se sabe, então nada de diarizar.
    let me = result.me_audio_f32()?;
    if !me.is_empty() {
        let cfg = FinalConfig {
            name: "eu".into(),
            lang: opts.lang.clone(),
            ..FinalConfig::meeting_final(vad_model)
        };
        let out = pipeline::run(&engine, &me, &cfg, &opts.context, None, None)?;
        log_report(meeting_id, "eu", &out);
        let eu = meeting::Speaker::Me.label();
        rows.extend(
            out.utterances
                .iter()
                .map(|u| (eu.clone(), u.start_secs, u.end_secs, u.text.clone())),
        );
    }

    anyhow::ensure!(
        !rows.is_empty(),
        "o passe final não produziu nenhuma fala (áudio sem voz?)"
    );
    rows.sort_by(|a, b| a.1.total_cmp(&b.1));

    let store = open_store()?;
    let n = store.replace_segments(meeting_id, &rows)?;
    rewrite_markdown(&store, meeting_id);
    Ok(Some(n))
}

fn log_report(meeting_id: i64, canal: &str, out: &FinalTranscript) {
    let r = &out.report;
    tracing::info!(
        meeting_id,
        canal,
        audio_secs = r.audio_secs,
        rtf = r.realtime_factor,
        vad_secs = r.timings.vad_secs,
        asr_secs = r.timings.asr_secs,
        diarize_secs = r.timings.diarize_secs,
        janelas = r.vad.windows,
        fala_secs = r.vad.speech_secs,
        silencio_secs = r.vad.discarded_silence_secs,
        segmentos = r.asr.segments,
        palavras = r.asr.words,
        logprob = r.asr.avg_logprob,
        falantes = r.speakers.speakers,
        trocas = r.speakers.switches,
        "passe final do canal"
    );
    if let Some(d) = &r.diarization {
        tracing::info!(
            meeting_id,
            canal,
            grupos_brutos = d.raw_clusters,
            falantes = d.speakers,
            absorvidos = d.absorbed_clusters,
            turnos = d.turns,
            turnos_curtos = d.very_short_turns,
            "diarização do canal"
        );
        for w in &d.warnings {
            tracing::warn!(meeting_id, canal, "diarização: {w}");
        }
    }
}
