//! isper-diarize — "quem falou o quê" (Fase 4), 100% local via sherpa-onnx.
//!
//! Pipeline clássico em dois modelos ONNX:
//! - **pyannote segmentation 3.0** acha os trechos de fala e as trocas de voz;
//! - **3D-Speaker (ERes2Net)** extrai a "impressão digital" de cada trecho,
//!   que é agrupada por similaridade → cada grupo é um falante.
//!
//! Roda como pós-processamento ao encerrar a reunião, sobre o áudio dos
//! "Participantes" (o loopback). Os modelos (~45 MB) são baixados uma vez
//! pelo gerenciador de modelos. O crate pesado (`sherpa-rs`) fica isolado
//! aqui — o `isper-core` não depende dele.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum DiarizeError {
    #[error("modelos de diarização não instalados — baixe em Configurações → Reuniões")]
    ModelsMissing,
    #[error("erro de diarização: {0}")]
    Engine(String),
    #[error(transparent)]
    Download(#[from] isper_models::ModelsError),
}

pub type Result<T> = std::result::Result<T, DiarizeError>;

pub const SEGMENTATION_FILE: &str = "pyannote-segmentation-3.0.onnx";
const SEGMENTATION_URL: &str = "https://huggingface.co/csukuangfj/sherpa-onnx-pyannote-segmentation-3-0/resolve/main/model.onnx";
pub const EMBEDDING_FILE: &str = "3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx";
const EMBEDDING_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx";
/// Tamanho aproximado dos dois modelos juntos, para a UI.
pub const APPROX_MB: u32 = 45;
/// Threshold padrão do agrupamento de falantes (ver `diarize`). Menor = mais
/// clusters. Na fixture de duas vozes sintéticas, 0.2 acertou e 0.4 dividiu
/// a mesma voz em duas; 0.3 é o meio-termo até calibrar com vozes reais.
pub const DEFAULT_THRESHOLD: f32 = 0.3;

/// Um turno de fala: `[start, end)` em segundos e o índice do falante.
#[derive(Debug, Clone, Copy)]
pub struct SpeakerTurn {
    pub start: f32,
    pub end: f32,
    pub speaker: usize,
}

/// `%LOCALAPPDATA%\ISPer\models\diarize`
pub fn models_dir() -> Result<PathBuf> {
    let dir = isper_models::models_dir()?.join("diarize");
    std::fs::create_dir_all(&dir).map_err(isper_models::ModelsError::Io)?;
    Ok(dir)
}

fn model_paths() -> Result<(PathBuf, PathBuf)> {
    let dir = models_dir()?;
    Ok((dir.join(SEGMENTATION_FILE), dir.join(EMBEDDING_FILE)))
}

pub fn models_installed() -> bool {
    model_paths()
        .map(|(s, e)| s.exists() && e.exists())
        .unwrap_or(false)
}

/// Baixa os dois modelos (pula os já presentes). `on_progress(nome, feito, total)`.
pub fn download_models(on_progress: &mut dyn FnMut(&str, u64, u64)) -> Result<()> {
    let (seg, emb) = model_paths()?;
    for (path, url, name) in [
        (seg, SEGMENTATION_URL, SEGMENTATION_FILE),
        (emb, EMBEDDING_URL, EMBEDDING_FILE),
    ] {
        if path.exists() {
            continue;
        }
        tracing::info!("baixando {name}");
        isper_models::download_asset(url, &path, None, None, &mut |done, total| {
            on_progress(name, done, total)
        })?;
    }
    Ok(())
}

/// Separa os falantes de um áudio 16 kHz mono f32. O número de falantes é
/// descoberto por agrupamento (threshold), não precisa ser informado.
pub fn diarize(samples_16k: &[f32]) -> Result<Vec<SpeakerTurn>> {
    let (seg, emb) = model_paths()?;
    if !seg.exists() || !emb.exists() {
        return Err(DiarizeError::ModelsMissing);
    }
    // Threshold do agrupamento: menor = mais falantes distintos (vozes
    // parecidas se separam), maior = mais conservador. Ajustável por env
    // para calibrar com reuniões reais sem recompilar.
    let threshold = std::env::var("ISPER_DIARIZE_THRESHOLD")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(DEFAULT_THRESHOLD);
    let config = sherpa_rs::diarize::DiarizeConfig {
        // -1 = número de falantes desconhecido → agrupa por similaridade.
        num_clusters: Some(-1),
        threshold: Some(threshold),
        // Ignora "falas" de menos de 300 ms e emenda pausas curtas (<0,5 s).
        min_duration_on: Some(0.3),
        min_duration_off: Some(0.5),
        provider: None,
        debug: false,
    };
    let mut engine = sherpa_rs::diarize::Diarize::new(&seg, &emb, config)
        .map_err(|e| DiarizeError::Engine(e.to_string()))?;
    let segments = engine
        .compute(samples_16k.to_vec(), None)
        .map_err(|e| DiarizeError::Engine(e.to_string()))?;
    Ok(segments
        .into_iter()
        .map(|s| SpeakerTurn {
            start: s.start,
            end: s.end,
            speaker: s.speaker.max(0) as usize,
        })
        .collect())
}
