//! isper-diarize — "quem falou o quê" (Fase 4), 100% local via sherpa-onnx.
//!
//! Pipeline clássico em dois modelos ONNX:
//! - **pyannote segmentation 3.0** acha os trechos de fala e as trocas de voz;
//! - um modelo de **speaker embedding** extrai a "impressão digital" de cada
//!   trecho, que é agrupada por similaridade → cada grupo é um falante.
//!
//! Os modelos (~45 MB) são baixados uma vez pelo gerenciador de modelos. O
//! crate pesado (`sherpa-onnx`, o oficial da k2-fsa) fica isolado aqui — o
//! `isper-core` não depende dele.
//!
//! O mesmo código roda no desktop e no celular (Fase 9.1): quem não usa as
//! pastas padrão do PC passa os caminhos dos modelos
//! ([`diarize_with_models`], [`download_models_to`]).
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
//!
//! E um terceiro, desde a Fase 9.4: a **conferência com a voz inteira**. Com
//! ligação completa, basta um par de trechos destoante para o agrupamento
//! partir a mesma voz, e num áudio curto de uma pessoa só (uma gravação no
//! celular) os dois pedaços ficam grandes demais para a absorção. Depois da
//! limpeza, cada falante ganha uma impressão de voz tirada de até
//! [`VOICE_SECS`] da fala dele — bem mais estável que a de um trecho —, e
//! falantes com a mesma voz viram um. É o mesmo passo que o 3D-Speaker, de
//! onde vem o modelo de impressão de voz, aplica depois do agrupamento; o
//! limiar foi calibrado ([`DEFAULT_SAME_VOICE`]).

use std::path::{Path, PathBuf};
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

/// Semelhança (cosseno das impressões de voz inteiras) a partir da qual dois
/// falantes são a mesma pessoa ([`DiarizeOptions::same_voice_similarity`]).
///
/// Calibrado em 27/09/2026 com o exemplo `conferir_vozes` do `isper-cli`
/// (metade da fala de cada um contra a outra metade, e falante contra
/// falante, nos turnos de referência):
///
/// | caso | semelhança |
/// |---|---|
/// | mesma voz, gravação real no celular (grupos de 3 a 7 s) | 0,54 a 0,71 |
/// | mesma voz, voz sintética | 0,80 a 0,98 |
/// | a mesma voz sintética a ±8 % de velocidade (as três "pessoas" da `reuniao-sintetica`) | 0,44 a 0,50 |
/// | vozes diferentes | 0,14 a 0,27 |
///
/// 0,6 fica 0,1 acima do pior caso de vozes diferentes. Na dúvida, não junta:
/// juntar duas pessoas apaga quem disse o quê, e uma pessoa partida em duas
/// se conserta renomeando uma delas.
pub const DEFAULT_SAME_VOICE: f32 = 0.6;

/// Quanto da fala de cada falante entra na impressão de voz da conferência.
pub const VOICE_SECS: f32 = 20.0;

/// Abaixo disto de fala, a impressão de voz não vale nada: com 1 s, trechos
/// da mesma pessoa deram semelhança perto de zero. O falante fica fora da
/// conferência.
pub const MIN_VOICE_SECS: f32 = 3.0;

/// Turnos mais curtos que isto ficam fora da impressão de voz. É neles que
/// moram o toque no botão, a respiração e o "hum" — trechos que o segmentador
/// marca como fala e que puxam a impressão para longe da voz: numa gravação
/// de uma pessoa só, metade da fala contra a outra deu 0,46 com eles dentro e
/// de 0,54 a 0,65 sem eles.
pub const MIN_TURN_SECS: f32 = 1.5;

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
    /// Threads do ONNX Runtime na segmentação e nos embeddings. 0 = escolha
    /// automática ([`auto_threads`]). O `sherpa-rs` fixava 1, e era por isso
    /// que 30 min de reunião levavam 12,8 min para separar os falantes.
    pub threads: usize,
    /// Na conferência com a voz inteira, dois falantes cujas impressões de
    /// voz têm semelhança (cosseno) a partir disto viram um. 0 desliga a
    /// conferência. Não roda com [`Self::num_speakers`]: quem disse quantos
    /// são já decidiu.
    pub same_voice_similarity: f32,
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
            threads: 0,
            same_voice_similarity: DEFAULT_SAME_VOICE,
        }
    }
}

/// Threads quando [`DiarizeOptions::threads`] é 0: metade dos núcleos lógicos
/// que o sistema oferece, entre 1 e [`MAX_AUTO_THREADS`].
///
/// Medido no corpus de 190 s num Ryzen 7 7735HS (8 núcleos, 16 threads):
/// 1 thread 63,6 s · 4 → 30,8 s · 8 → 22,7 s · 16 → 31,3 s. Passar dos núcleos
/// físicos piora (o SMT disputa as mesmas unidades de ponto flutuante), e a
/// metade dos lógicos é a melhor aproximação portátil dos físicos. Num
/// celular de 8 núcleos sem SMT, dá 4, que são em geral os núcleos rápidos.
pub fn auto_threads() -> usize {
    let logical = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    (logical / 2).clamp(1, MAX_AUTO_THREADS)
}

/// Teto da escolha automática de threads.
pub const MAX_AUTO_THREADS: usize = 8;

impl DiarizeOptions {
    /// Lê `ISPER_DIARIZE_THRESHOLD`, `ISPER_DIARIZE_SPEAKERS`,
    /// `ISPER_DIARIZE_THREADS` e `ISPER_DIARIZE_SAME_VOICE` — calibrar e medir
    /// numa reunião real sem recompilar continua possível.
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
        if let Some(n) = std::env::var("ISPER_DIARIZE_THREADS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
        {
            o.threads = n;
        }
        if let Some(s) = std::env::var("ISPER_DIARIZE_SAME_VOICE")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
        {
            o.same_voice_similarity = s;
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
    Ok(model_paths_in(&models_dir()?))
}

/// Os caminhos dos dois modelos (segmentação, embedding) dentro de `dir`,
/// existam eles ou não.
pub fn model_paths_in(dir: &Path) -> (PathBuf, PathBuf) {
    (dir.join(SEGMENTATION_FILE), dir.join(EMBEDDING_FILE))
}

pub fn models_installed() -> bool {
    model_paths()
        .map(|(s, e)| s.exists() && e.exists())
        .unwrap_or(false)
}

/// Baixa os dois modelos (pula os já presentes). `on_progress(nome, feito, total)`.
pub fn download_models(on_progress: &mut dyn FnMut(&str, u64, u64)) -> Result<()> {
    download_models_to(&models_dir()?, on_progress)
}

/// Como [`download_models`], para uma pasta escolhida por quem chama (o
/// celular guarda os modelos na pasta do próprio app).
pub fn download_models_to(dir: &Path, on_progress: &mut dyn FnMut(&str, u64, u64)) -> Result<()> {
    let (seg, emb) = model_paths_in(dir);
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
    diarize_with_models(&seg, &emb, samples_16k, opts)
}

/// Como [`diarize_with`], com os modelos em caminhos escolhidos por quem
/// chama — o celular não tem as pastas do PC.
pub fn diarize_with_models(
    segmentation: &Path,
    embedding: &Path,
    samples_16k: &[f32],
    opts: &DiarizeOptions,
) -> Result<DiarizeOutcome> {
    if !segmentation.exists() || !embedding.exists() {
        return Err(DiarizeError::ModelsMissing);
    }
    let started = Instant::now();
    let threads = if opts.threads == 0 {
        auto_threads()
    } else {
        opts.threads
    };
    let path = |p: &Path| Some(p.to_string_lossy().into_owned());
    let config = sherpa_onnx::OfflineSpeakerDiarizationConfig {
        segmentation: sherpa_onnx::OfflineSpeakerSegmentationModelConfig {
            pyannote: sherpa_onnx::OfflineSpeakerSegmentationPyannoteModelConfig {
                model: path(segmentation),
                ..Default::default()
            },
            num_threads: threads as i32,
            ..Default::default()
        },
        embedding: sherpa_onnx::SpeakerEmbeddingExtractorConfig {
            model: path(embedding),
            num_threads: threads as i32,
            ..Default::default()
        },
        clustering: sherpa_onnx::FastClusteringConfig {
            // -1 = número de falantes desconhecido → agrupa por similaridade.
            num_clusters: opts.num_speakers.map(|n| n as i32).unwrap_or(-1),
            threshold: opts.threshold,
            ..Default::default()
        },
        min_duration_on: opts.min_duration_on,
        min_duration_off: opts.min_duration_off,
    };
    // `create` devolve None quando um modelo não carrega (arquivo corrompido,
    // formato errado); o sherpa-onnx escreve o motivo no stderr.
    let engine = sherpa_onnx::OfflineSpeakerDiarization::create(&config).ok_or_else(|| {
        DiarizeError::Engine("o sherpa-onnx não carregou os modelos de diarização".into())
    })?;
    let result = engine.process(samples_16k).ok_or_else(|| {
        DiarizeError::Engine("o sherpa-onnx não conseguiu processar o áudio".into())
    })?;
    let raw: Vec<SpeakerTurn> = result
        .sort_by_start_time()
        .into_iter()
        .map(|s| SpeakerTurn {
            start: s.start,
            end: s.end,
            speaker: s.speaker.max(0) as u32,
        })
        .collect();

    let (mut turns, mut metrics) = postprocess(raw, opts);

    // A conferência com a voz inteira: a mesma pessoa partida em dois grupos
    // vira uma. Se o extrator falhar, fica o resultado do agrupamento.
    if opts.num_speakers.is_none() && metrics.speakers >= 2 {
        match voice_similarities(embedding, samples_16k, &turns, threads) {
            Ok(similarities) => {
                tracing::info!(pares = ?similarities, "semelhança entre as vozes");
                if opts.same_voice_similarity > 0.0 {
                    let mut ids: Vec<u32> = turns.iter().map(|t| t.speaker).collect();
                    ids.sort_unstable();
                    ids.dedup();
                    let into = postprocess::same_voice_groups(
                        &ids,
                        &similarities,
                        opts.same_voice_similarity,
                    );
                    if into.iter().any(|(from, to)| from != to) {
                        turns = postprocess::apply_same_voice(turns, &into, &mut metrics, opts);
                    }
                }
                metrics.voice_similarities = similarities;
            }
            Err(e) => tracing::warn!("a conferência das vozes falhou ({e}): fica o agrupamento"),
        }
    }
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
        same_voice = metrics.merged_same_voice,
        median_turn_secs = metrics.median_turn_secs,
        very_short_turns = metrics.very_short_turns,
        threads,
        secs = metrics.elapsed_secs,
        "diarização concluída"
    );

    Ok(DiarizeOutcome {
        turns,
        metrics,
        warnings,
    })
}

/// A semelhança (cosseno, de −1 a 1) entre as vozes de cada par de falantes de
/// `turns`, com a impressão de voz de cada um tirada de até [`VOICE_SECS`] da
/// fala dele. Um falante com menos de [`MIN_VOICE_SECS`] de fala fica fora dos
/// pares.
pub fn voice_similarities(
    embedding: &Path,
    samples_16k: &[f32],
    turns: &[SpeakerTurn],
    threads: usize,
) -> Result<Vec<(u32, u32, f32)>> {
    let config = sherpa_onnx::SpeakerEmbeddingExtractorConfig {
        model: Some(embedding.to_string_lossy().into_owned()),
        num_threads: threads.max(1) as i32,
        ..Default::default()
    };
    let extractor = sherpa_onnx::SpeakerEmbeddingExtractor::create(&config).ok_or_else(|| {
        DiarizeError::Engine("o sherpa-onnx não carregou o modelo de impressão de voz".into())
    })?;
    let prints = voice_prints(&extractor, samples_16k, turns);
    let mut out = Vec::new();
    for (i, (a, pa)) in prints.iter().enumerate() {
        for (b, pb) in &prints[i + 1..] {
            out.push((*a, *b, cosine(pa, pb)));
        }
    }
    Ok(out)
}

/// A impressão de voz de cada falante, com até [`VOICE_SECS`] da fala dele:
/// os turnos mais longos primeiro, que são os mais limpos, e nenhum abaixo de
/// [`MIN_TURN_SECS`].
fn voice_prints(
    extractor: &sherpa_onnx::SpeakerEmbeddingExtractor,
    samples_16k: &[f32],
    turns: &[SpeakerTurn],
) -> Vec<(u32, Vec<f32>)> {
    const RATE: f32 = 16_000.0;
    let mut ids: Vec<u32> = turns.iter().map(|t| t.speaker).collect();
    ids.sort_unstable();
    ids.dedup();
    let mut out = Vec::new();
    for id in ids {
        let mut own: Vec<&SpeakerTurn> = turns
            .iter()
            .filter(|t| t.speaker == id && t.secs() >= MIN_TURN_SECS)
            .collect();
        own.sort_by(|a, b| b.secs().total_cmp(&a.secs()));
        let mut audio: Vec<f32> = Vec::new();
        for t in own {
            let a = ((t.start.max(0.0) * RATE) as usize).min(samples_16k.len());
            let b = ((t.end.max(0.0) * RATE) as usize).min(samples_16k.len());
            if b > a {
                audio.extend_from_slice(&samples_16k[a..b]);
            }
            if audio.len() as f32 >= VOICE_SECS * RATE {
                break;
            }
        }
        if (audio.len() as f32) < MIN_VOICE_SECS * RATE {
            continue;
        }
        let Some(stream) = extractor.create_stream() else {
            continue;
        };
        stream.accept_waveform(RATE as i32, &audio);
        stream.input_finished();
        if !extractor.is_ready(&stream) {
            continue;
        }
        if let Some(e) = extractor.compute(&stream) {
            out.push((id, e));
        }
    }
    out
}

/// Cosseno entre dois vetores (0 se algum for nulo).
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}
