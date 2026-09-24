//! `isper-cli bench` — o laboratório de medição do pipeline.
//!
//! O princípio é "medir antes de otimizar": qualquer mudança no pipeline
//! precisa ser comparável com o que havia antes, sobre o MESMO áudio, com os
//! parâmetros gravados junto do resultado.
//!
//! ```text
//! isper-cli bench reuniao.wav --config baseline
//! isper-cli bench reuniao.wav --config final --glossary glossario.json
//! isper-cli compare bench/baseline.report.json bench/final.report.json
//! isper-cli score --ref corrigido.txt --hyp bench/final.txt
//! ```
//!
//! Cada rodada deixa em disco: o relatório (JSON com TODOS os parâmetros), a
//! transcrição bruta, a normalizada, a transcrição com falantes e as palavras
//! com timestamp — o suficiente para auditar um número sem rodar de novo.

use std::path::{Path, PathBuf};

use anyhow::Context;
use isper_core::align::SpeakerTurn;
use isper_core::context::MeetingContext;
use isper_core::metrics::{self, DiarizeStats, Normalization, PipelineReport};
use isper_core::pipeline::{
    self, Diarizer, DiarizerOutput, FinalConfig, FinalTranscript, Segmentation,
};
use isper_core::{EngineOptions, WhisperEngine};

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
            stats: DiarizeStats {
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

/// Argumentos de uma rodada.
pub struct BenchArgs {
    pub audio: PathBuf,
    /// "baseline", "final" ou o caminho de um JSON de [`FinalConfig`].
    pub config: String,
    /// Nome dos arquivos de saída (padrão: o da configuração).
    pub name: Option<String>,
    pub out_dir: PathBuf,
    pub model: PathBuf,
    pub lang: String,
    /// Glossário: `.json` de [`MeetingContext`] ou `.txt` com um termo por linha.
    pub glossary: Option<PathBuf>,
    pub diarize: bool,
    /// Número de participantes, quando conhecido — muda o agrupamento de
    /// "descubra quantos" para "corte em exatamente N".
    pub speakers: Option<u32>,
    pub diarize_threshold: Option<f32>,
    /// Transcrição corrigida à mão, para WER/CER.
    pub reference: Option<PathBuf>,
    /// Turnos de referência (`início<TAB>fim<TAB>falante` por linha), para DER.
    pub reference_turns: Option<PathBuf>,
}

pub fn run(args: &BenchArgs) -> anyhow::Result<PipelineReport> {
    let decoded = isper_core::decode::decode_to_16k(&args.audio, None, None)
        .with_context(|| format!("falha ao ler {}", args.audio.display()))?;
    println!(
        "áudio: {:.1}s @ {} Hz, {} canal(is)",
        decoded.duration_secs(),
        decoded.source_rate,
        decoded.source_channels
    );
    let samples = decoded.samples_16k;

    let mut cfg = load_config(&args.config)?;
    cfg.lang = args.lang.clone();
    if let Some(n) = &args.name {
        cfg.name = n.clone();
    }
    let ctx = load_context(args.glossary.as_deref())?;

    // DTW só compensa quando o perfil vai pedir timestamps por token: ele
    // guarda a atenção cruzada e custa memória à toa no resto.
    let opts = EngineOptions {
        dtw: cfg.decode.token_timestamps,
    };
    println!("carregando modelo {} ...", args.model.display());
    let engine = WhisperEngine::new_with(&args.model, &opts)?;
    if opts.dtw && !engine.has_dtw() {
        println!("aviso: DTW indisponível para este modelo — seguindo sem ele");
    }

    let diarizer = args.diarize.then(|| SherpaDiarizer {
        opts: isper_diarize::DiarizeOptions {
            num_speakers: args.speakers,
            threshold: args
                .diarize_threshold
                .unwrap_or(isper_diarize::DEFAULT_THRESHOLD),
            ..isper_diarize::DiarizeOptions::default()
        },
    });
    if args.diarize && !isper_diarize::models_installed() {
        anyhow::bail!(
            "modelos de diarização não instalados — rode `isper-cli models download-diarize` ou passe --no-diarize"
        );
    }
    ensure_vad_model(&cfg)?;

    println!("rodando configuração '{}'...", cfg.name);
    let out = pipeline::run(
        &engine,
        &samples,
        &cfg,
        &ctx,
        diarizer.as_ref().map(|d| d as &dyn Diarizer),
        Some(&|feitas, total| {
            if total > 0 && (feitas % 10 == 0 || feitas == total) {
                println!("  janela {feitas}/{total}");
            }
        }),
    )?;

    write_outputs(args, &cfg, &out)?;
    print_summary(&out.report);
    score_against_reference(args, &out)?;
    Ok(out.report)
}

/// O modelo de VAD tem 0,9 MB: baixar na hora é mais gentil que falhar.
fn ensure_vad_model(cfg: &FinalConfig) -> anyhow::Result<()> {
    let Segmentation::Vad { model, .. } = &cfg.segmentation else {
        return Ok(());
    };
    if model.exists() {
        return Ok(());
    }
    println!(
        "baixando o modelo de VAD ({} MB)...",
        isper_models::VAD_APPROX_MB
    );
    let baixado = isper_models::download_vad(&mut |_, _| {})?;
    anyhow::ensure!(
        baixado == *model,
        "o modelo de VAD foi para {} mas a configuração aponta para {}",
        baixado.display(),
        model.display()
    );
    Ok(())
}

fn load_config(spec: &str) -> anyhow::Result<FinalConfig> {
    match spec {
        "baseline" => Ok(FinalConfig::baseline()),
        "final" => Ok(FinalConfig::meeting_final(isper_models::vad_path()?)),
        path => {
            let texto = std::fs::read_to_string(path).with_context(|| {
                format!("configuração '{path}' não é 'baseline', 'final' nem um arquivo legível")
            })?;
            serde_json::from_str(&texto).with_context(|| format!("JSON inválido em {path}"))
        }
    }
}

fn load_context(path: Option<&Path>) -> anyhow::Result<MeetingContext> {
    let Some(path) = path else {
        return Ok(MeetingContext::default());
    };
    let texto = std::fs::read_to_string(path)
        .with_context(|| format!("falha ao ler {}", path.display()))?;
    if path.extension().is_some_and(|e| e == "json") {
        return serde_json::from_str(&texto)
            .with_context(|| format!("JSON inválido em {}", path.display()));
    }
    let termos: Vec<String> = texto
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect();
    Ok(MeetingContext::from_dictionary(&termos))
}

fn write_outputs(args: &BenchArgs, cfg: &FinalConfig, out: &FinalTranscript) -> anyhow::Result<()> {
    std::fs::create_dir_all(&args.out_dir)?;
    let base = args.out_dir.join(&cfg.name);
    let write = |suffix: &str, content: &str| -> anyhow::Result<PathBuf> {
        let p = PathBuf::from(format!("{}{suffix}", base.display()));
        std::fs::write(&p, content)?;
        Ok(p)
    };

    write(".report.json", &out.report.to_json())?;
    write(".config.json", &serde_json::to_string_pretty(cfg)?)?;
    write(".raw.txt", &out.raw_text)?;
    write(".norm.txt", &out.normalized_text)?;

    let mut md = String::new();
    for u in &out.utterances {
        md.push_str(&format!(
            "[{}] {}: {}\n",
            fmt_ts(u.start_secs),
            u.speaker
                .map(|s| format!("Participante {}", s + 1))
                .unwrap_or_else(|| "Participantes".into()),
            u.text
        ));
    }
    let transcript = write(".transcript.txt", &md)?;

    let mut tsv = String::from("inicio\tfim\tfalante\tprob\tpalavra\n");
    for w in &out.words {
        tsv.push_str(&format!(
            "{:.2}\t{:.2}\t{}\t{:.2}\t{}\n",
            w.word.start_secs,
            w.word.end_secs,
            w.speaker
                .map(|s| (s + 1).to_string())
                .unwrap_or_else(|| "-".into()),
            w.word.prob,
            w.word.text
        ));
    }
    write(".words.tsv", &tsv)?;

    println!("saída em {}", args.out_dir.display());
    println!("  transcrição: {}", transcript.display());
    Ok(())
}

fn print_summary(r: &PipelineReport) {
    println!();
    println!("== {} ==", r.config);
    println!(
        "modelo            {} ({}{})",
        r.model,
        r.device,
        if r.dtw { ", dtw" } else { "" }
    );
    println!(
        "áudio             {:.1}s | processamento {:.1}s | RTF {:.2}",
        r.audio_secs, r.timings.total_secs, r.realtime_factor
    );
    println!(
        "etapas            vad {:.1}s · asr {:.1}s · diar {:.1}s · align {:.1}s",
        r.timings.vad_secs, r.timings.asr_secs, r.timings.diarize_secs, r.timings.align_secs
    );
    println!(
        "vad               {} regiões · fala {:.1}s · silêncio descartado {:.1}s · {} janelas (mediana {:.1}s)",
        r.vad.regions,
        r.vad.speech_secs,
        r.vad.discarded_silence_secs,
        r.vad.windows,
        r.vad.median_window_secs
    );
    println!(
        "asr               {} segmentos · {} palavras · logprob médio {:.2} · {} descartados · {} janelas com contexto",
        r.asr.segments,
        r.asr.words,
        r.asr.avg_logprob,
        r.asr.dropped_segments,
        r.asr.windows_with_context
    );
    println!(
        "                  segmentos <1s: {} · mediana {:.1}s · janelas vazias {} · falhas {}",
        r.asr.segments_under_1s,
        r.asr.median_segment_secs,
        r.asr.empty_windows,
        r.asr.failed_windows
    );
    if let Some(d) = &r.diarization {
        println!(
            "diarização        {} grupos brutos → {} falantes ({} absorvidos) · {} turnos · mediana {:.1}s · curtos {}",
            d.raw_clusters,
            d.speakers,
            d.absorbed_clusters,
            d.turns,
            d.median_turn_secs,
            d.very_short_turns
        );
        for w in &d.warnings {
            println!("  AVISO: {w}");
        }
    }
    println!(
        "falantes          {} · trocas {} · mediana de fala {:.1}s · falas <0,5s {} · sem dono {}",
        r.speakers.speakers,
        r.speakers.switches,
        r.speakers.median_turn_secs,
        r.speakers.very_short_turns,
        r.speakers.unassigned
    );
}

fn score_against_reference(args: &BenchArgs, out: &FinalTranscript) -> anyhow::Result<()> {
    if let Some(path) = &args.reference {
        let referencia = std::fs::read_to_string(path)
            .with_context(|| format!("falha ao ler {}", path.display()))?;
        let n = Normalization::default();
        let w = metrics::wer(&referencia, &out.normalized_text, &n);
        let c = metrics::cer(&referencia, &out.normalized_text, &n);
        println!();
        println!(
            "WER               {:.2}% ({} sub · {} del · {} ins em {} palavras)",
            w.rate * 100.0,
            w.substitutions,
            w.deletions,
            w.insertions,
            w.reference_len
        );
        println!("CER               {:.2}%", c.rate * 100.0);
    }
    if let Some(path) = &args.reference_turns {
        let turnos = read_turns(path)?;
        let hipotese: Vec<SpeakerTurn> = out
            .utterances
            .iter()
            .filter_map(|u| {
                u.speaker.map(|s| SpeakerTurn {
                    start_secs: u.start_secs,
                    end_secs: u.end_secs,
                    speaker: s,
                })
            })
            .collect();
        let d = metrics::der(&turnos, &hipotese);
        println!(
            "DER               {:.2}% (omissão {:.1}s · falso alarme {:.1}s · confusão {:.1}s de {:.1}s)",
            d.rate * 100.0,
            d.missed_secs,
            d.false_alarm_secs,
            d.confusion_secs,
            d.reference_secs
        );
    }
    Ok(())
}

/// `início<TAB>fim<TAB>falante` por linha; `#` começa comentário.
pub(crate) fn read_turns(path: &Path) -> anyhow::Result<Vec<SpeakerTurn>> {
    let texto = std::fs::read_to_string(path)
        .with_context(|| format!("falha ao ler {}", path.display()))?;
    let mut out = Vec::new();
    let mut nomes: Vec<String> = Vec::new();
    for (i, linha) in texto.lines().enumerate() {
        let linha = linha.trim();
        if linha.is_empty() || linha.starts_with('#') {
            continue;
        }
        let campos: Vec<&str> = linha.split(['\t', ';']).map(str::trim).collect();
        anyhow::ensure!(
            campos.len() >= 3,
            "{}:{}: esperava início<TAB>fim<TAB>falante",
            path.display(),
            i + 1
        );
        let start: f32 = campos[0]
            .parse()
            .with_context(|| format!("linha {}", i + 1))?;
        let end: f32 = campos[1]
            .parse()
            .with_context(|| format!("linha {}", i + 1))?;
        let nome = campos[2].to_string();
        let id = match nomes.iter().position(|n| *n == nome) {
            Some(k) => k,
            None => {
                nomes.push(nome);
                nomes.len() - 1
            }
        };
        out.push(SpeakerTurn {
            start_secs: start,
            end_secs: end,
            speaker: id as u32,
        });
    }
    Ok(out)
}

/// Compara dois relatórios lado a lado — é o entregável "antes/depois".
pub fn compare(antes: &Path, depois: &Path) -> anyhow::Result<()> {
    let a: PipelineReport = serde_json::from_str(&std::fs::read_to_string(antes)?)
        .with_context(|| format!("JSON inválido em {}", antes.display()))?;
    let b: PipelineReport = serde_json::from_str(&std::fs::read_to_string(depois)?)
        .with_context(|| format!("JSON inválido em {}", depois.display()))?;

    println!("{:<26} {:>14} {:>14}   Δ", "", a.config, b.config);
    let linha_f = |nome: &str, x: f32, y: f32, casas: usize| {
        let d = y - x;
        println!(
            "{nome:<26} {x:>14.casas$} {y:>14.casas$}   {}{:.casas$}",
            if d >= 0.0 { "+" } else { "" },
            d
        );
    };
    let linha_i = |nome: &str, x: usize, y: usize| {
        let d = y as i64 - x as i64;
        println!(
            "{nome:<26} {x:>14} {y:>14}   {}{d}",
            if d >= 0 { "+" } else { "" }
        );
    };

    linha_f("duração do áudio (s)", a.audio_secs, b.audio_secs, 1);
    linha_f(
        "processamento (s)",
        a.timings.total_secs,
        b.timings.total_secs,
        1,
    );
    linha_f(
        "fator de tempo real",
        a.realtime_factor,
        b.realtime_factor,
        3,
    );
    linha_f("  vad (s)", a.timings.vad_secs, b.timings.vad_secs, 1);
    linha_f("  asr (s)", a.timings.asr_secs, b.timings.asr_secs, 1);
    linha_f(
        "  diarização (s)",
        a.timings.diarize_secs,
        b.timings.diarize_secs,
        1,
    );
    linha_i("janelas", a.vad.windows, b.vad.windows);
    linha_f(
        "silêncio descartado (s)",
        a.vad.discarded_silence_secs,
        b.vad.discarded_silence_secs,
        1,
    );
    linha_i("segmentos", a.asr.segments, b.asr.segments);
    linha_i(
        "segmentos < 1s",
        a.asr.segments_under_1s,
        b.asr.segments_under_1s,
    );
    linha_i("palavras", a.asr.words, b.asr.words);
    linha_f("logprob médio", a.asr.avg_logprob, b.asr.avg_logprob, 3);
    linha_i(
        "segmentos descartados",
        a.asr.dropped_segments,
        b.asr.dropped_segments,
    );
    if let (Some(x), Some(y)) = (&a.diarization, &b.diarization) {
        linha_i("grupos brutos", x.raw_clusters, y.raw_clusters);
        linha_i("falantes", x.speakers, y.speakers);
        linha_i("turnos", x.turns, y.turns);
        linha_f(
            "mediana do turno (s)",
            x.median_turn_secs,
            y.median_turn_secs,
            1,
        );
        linha_i("turnos < 0,5s", x.very_short_turns, y.very_short_turns);
    }
    linha_i(
        "falas por falante trocado",
        a.speakers.switches,
        b.speakers.switches,
    );
    linha_i(
        "falas sem dono",
        a.speakers.unassigned,
        b.speakers.unassigned,
    );
    Ok(())
}

/// WER/CER entre dois arquivos de texto, sem rodar nada.
pub fn score(reference: &Path, hypothesis: &Path, strip_accents: bool) -> anyhow::Result<()> {
    let r = std::fs::read_to_string(reference)
        .with_context(|| format!("falha ao ler {}", reference.display()))?;
    let h = std::fs::read_to_string(hypothesis)
        .with_context(|| format!("falha ao ler {}", hypothesis.display()))?;
    let n = Normalization {
        strip_accents,
        ..Normalization::default()
    };
    let w = metrics::wer(&r, &h, &n);
    let c = metrics::cer(&r, &h, &n);
    println!(
        "WER {:.2}%  ({} sub · {} del · {} ins em {} palavras)",
        w.rate * 100.0,
        w.substitutions,
        w.deletions,
        w.insertions,
        w.reference_len
    );
    println!(
        "CER {:.2}%  ({} erros em {} caracteres)",
        c.rate * 100.0,
        c.errors(),
        c.reference_len
    );
    Ok(())
}

fn fmt_ts(secs: f32) -> String {
    let s = secs.max(0.0) as u32;
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}
