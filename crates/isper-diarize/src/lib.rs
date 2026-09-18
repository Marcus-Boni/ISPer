//! isper-diarize — "quem falou o quê" (Fase 4), 100% local via sherpa-onnx.
//!
//! Pipeline clássico em dois modelos ONNX:
//! - **pyannote segmentation 3.0** acha os trechos de fala e as trocas de voz;
//! - um modelo de **speaker embedding** extrai a "impressão digital" de cada
//!   trecho, que é agrupada por similaridade → cada grupo é um falante.
//!
//! Os modelos (~45 MB) são baixados uma vez pelo gerenciador de modelos. O
//! crate pesado (`sherpa-rs`) fica isolado aqui — o `isper-core` não depende
//! dele.
//!
//! ## O que o agrupamento faz de verdade
//!
//! O sherpa-onnx usa **agrupamento hierárquico com ligação COMPLETA sobre
//! dissimilaridade de cosseno** (`fast-clustering.cc`), cortado por altura.
//! Ligação completa significa que dois grupos só se juntam se TODOS os pares
//! entre eles estiverem dentro do limiar — é o critério mais exigente que
//! existe. Num áudio de reunião (voz comprimida pelo Teams, trechos curtos,
//! fala sobreposta), a distância entre dois trechos da MESMA pessoa passa de
//! 0,3 com frequência. Com o limiar em 0,3, cada trecho vira seu próprio
//! grupo: é daí que vinham dezenas ou centenas de "participantes".
//!
//! Por isso, além do limiar (o default do sherpa-onnx é 0,5), este módulo
//! aplica dois cuidados que não dependem de calibrar número nenhum:
//!
//! - grupos com pouquíssimo tempo de fala são **absorvidos** pelo vizinho
//!   temporal ([`postprocess`]) — quem participa de uma reunião fala mais do
//!   que alguns segundos no total;
//! - uma contagem absurda de grupos **não é publicada em silêncio**: vira
//!   aviso e `reliable = false`, para quem chamou decidir (ver
//!   [`DiarizeOutcome`]).

use std::path::PathBuf;
use std::time::Instant;

mod postprocess;
pub use postprocess::{DiarizeMetrics, postprocess};

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

/// Limiar padrão do agrupamento.
///
/// É o default do próprio sherpa-onnx (`FastClusteringConfig::threshold`) e o
/// valor dos exemplos oficiais. O ISPer usava 0,3, calibrado numa fixture de
/// duas vozes sintéticas — o que não representa reunião real: com ligação
/// completa, 0,3 exige que todos os trechos de uma pessoa estejam a menos de
/// 0,3 de distância entre si, o que voz comprimida não cumpre.
pub const DEFAULT_THRESHOLD: f32 = 0.5;

/// Um turno de fala: `[start, end)` em segundos e o índice do falante.
///
/// `speaker` é `u32`: com `u8` o app precisava saturar em 255 e o sintoma de
/// um agrupamento quebrado virava um rótulo ("Participante 255") em vez de um
/// erro. Agora o número absurdo aparece nas métricas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeakerTurn {
    pub start: f32,
    pub end: f32,
    pub speaker: u32,
}

impl SpeakerTurn {
    pub fn secs(&self) -> f32 {
        (self.end - self.start).max(0.0)
    }
}

/// Parâmetros do agrupamento e das proteções.
#[derive(Debug, Clone, PartialEq)]
pub struct DiarizeOptions {
    /// Distância de corte do agrupamento. Maior = menos falantes.
    pub threshold: f32,
    /// Número de participantes, quando conhecido. Preenchido, o sherpa-onnx
    /// ignora o limiar e corta o dendrograma em exatamente N grupos — de
    /// longe o modo mais robusto, quando a informação existe.
    pub num_speakers: Option<u32>,
    /// Fala mais curta que isto é ignorada pelo segmentador (segundos).
    pub min_duration_on: f32,
    /// Pausa menor que isto não separa dois turnos (segundos).
    pub min_duration_off: f32,
    /// Tempo total de fala abaixo do qual um grupo não é um participante e
    /// é absorvido pelo vizinho. 0 desliga a absorção.
    pub min_speaker_secs: f32,
    /// Idem, como FRAÇÃO da fala total — o critério que não envelhece com a
    /// duração da reunião. Seis segundos são muito numa reunião de três
    /// minutos e nada numa de duas horas; vale o maior dos dois.
    pub min_speaker_share: f32,
    /// Idem para a contagem de turnos.
    pub min_speaker_turns: usize,
    /// Acima desta contagem de grupos, o resultado é marcado como duvidoso.
    pub max_plausible_speakers: usize,
}

impl Default for DiarizeOptions {
    fn default() -> Self {
        Self {
            threshold: DEFAULT_THRESHOLD,
            num_speakers: None,
            // Defaults antigos do ISPer, mantidos: ignoram "falas" de menos de
            // 300 ms e emendam pausas de até 0,5 s.
            min_duration_on: 0.3,
            min_duration_off: 0.5,
            min_speaker_secs: 6.0,
            min_speaker_share: 0.02,
            min_speaker_turns: 2,
            max_plausible_speakers: 12,
        }
    }
}

impl DiarizeOptions {
    /// Lê `ISPER_DIARIZE_THRESHOLD` e `ISPER_DIARIZE_SPEAKERS` — calibrar numa
    /// reunião real sem recompilar continua possível.
    pub fn from_env() -> Self {
        let mut o = Self::default();
        if let Some(t) = std::env::var("ISPER_DIARIZE_THRESHOLD")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
        {
            o.threshold = t;
        }
        if let Some(n) = std::env::var("ISPER_DIARIZE_SPEAKERS")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|n| *n > 0)
        {
            o.num_speakers = Some(n);
        }
        o
    }
}

/// O resultado completo: turnos, o que aconteceu e se dá para confiar.
#[derive(Debug, Clone)]
pub struct DiarizeOutcome {
    pub turns: Vec<SpeakerTurn>,
    pub metrics: DiarizeMetrics,
    /// Motivos pelos quais o resultado é duvidoso (vazio = tudo bem).
    pub warnings: Vec<String>,
}

impl DiarizeOutcome {
    /// `false` quando o agrupamento produziu algo implausível. Quem chamou
    /// deve preferir os rótulos genéricos a publicar isto.
    pub fn reliable(&self) -> bool {
        self.warnings.is_empty()
    }
}

/// `%LOCALAPPDATA%\com.isper.desktop\models\diarize`
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

/// Separa os falantes de um áudio 16 kHz mono f32, com os parâmetros padrão
/// (e o que estiver nas variáveis de ambiente).
///
/// **Importante**: passe o áudio CONTÍNUO da reunião. Concatenar só os
/// trechos "não silenciosos" cria emendas onde uma pessoa vira outra no meio
/// da janela do segmentador — foi o que fazia a contagem de falantes explodir.
pub fn diarize(samples_16k: &[f32]) -> Result<Vec<SpeakerTurn>> {
    Ok(diarize_with(samples_16k, &DiarizeOptions::from_env())?.turns)
}

/// Como [`diarize`], devolvendo também as métricas e os avisos.
pub fn diarize_with(samples_16k: &[f32], opts: &DiarizeOptions) -> Result<DiarizeOutcome> {
    let (seg, emb) = model_paths()?;
    if !seg.exists() || !emb.exists() {
        return Err(DiarizeError::ModelsMissing);
    }
    let started = Instant::now();
    let config = sherpa_rs::diarize::DiarizeConfig {
        // -1 = número de falantes desconhecido → agrupa por similaridade.
        num_clusters: Some(opts.num_speakers.map(|n| n as i32).unwrap_or(-1)),
        threshold: Some(opts.threshold),
        min_duration_on: Some(opts.min_duration_on),
        min_duration_off: Some(opts.min_duration_off),
        provider: None,
        debug: false,
    };
    let mut engine = sherpa_rs::diarize::Diarize::new(&seg, &emb, config)
        .map_err(|e| DiarizeError::Engine(e.to_string()))?;
    let segments = engine
        .compute(samples_16k.to_vec(), None)
        .map_err(|e| DiarizeError::Engine(e.to_string()))?;
    let raw: Vec<SpeakerTurn> = segments
        .into_iter()
        .map(|s| SpeakerTurn {
            start: s.start,
            end: s.end,
            speaker: s.speaker.max(0) as u32,
        })
        .collect();

    let (turns, mut metrics) = postprocess(raw, opts);
    metrics.elapsed_secs = started.elapsed().as_secs_f32();
    metrics.audio_secs = samples_16k.len() as f32 / 16_000.0;

    let mut warnings = Vec::new();
    if metrics.raw_clusters > opts.max_plausible_speakers {
        warnings.push(format!(
            "{} grupos de voz em {:.0} min de áudio — acima do plausível ({}); \
             o agrupamento provavelmente não convergiu",
            metrics.raw_clusters,
            metrics.audio_secs / 60.0,
            opts.max_plausible_speakers
        ));
    }
    if metrics.speakers > opts.max_plausible_speakers {
        warnings.push(format!(
            "{} falantes após a limpeza — acima do plausível ({})",
            metrics.speakers, opts.max_plausible_speakers
        ));
    }
    for w in &warnings {
        tracing::warn!("diarização: {w}");
    }
    tracing::info!(
        raw_clusters = metrics.raw_clusters,
        speakers = metrics.speakers,
        turns = metrics.turns,
        absorbed = metrics.absorbed_clusters,
        median_turn_secs = metrics.median_turn_secs,
        very_short_turns = metrics.very_short_turns,
        secs = metrics.elapsed_secs,
        "diarização concluída"
    );

    Ok(DiarizeOutcome {
        turns,
        metrics,
        warnings,
    })
}
