//! isper-models — gerenciador de modelos e ativos do ISPer (Fase 3).
//!
//! - Catálogo dos modelos Whisper (ggml) com rótulo, tamanho e recomendação;
//! - Download com progresso para `%LOCALAPPDATA%\ISPer\models` (arquivos
//!   grandes ficam no LocalAppData, não no Roaming);
//! - **Integridade**: o SHA-256 esperado vem do próprio Hugging Face (o
//!   `lfs.oid` publicado pela API do repositório) e é conferido durante o
//!   download — nada de checksum hardcoded que envelhece;
//! - Download genérico de ativos (usado pelos modelos de diarização).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum ModelsError {
    #[error("modelo desconhecido: {0}")]
    NotInCatalog(String),
    #[error("erro HTTP: {0}")]
    Http(String),
    #[error("checksum inválido para {file}: esperado {expected}, obtido {got} — arquivo descartado")]
    Checksum {
        file: String,
        expected: String,
        got: String,
    },
    #[error("LOCALAPPDATA não definido")]
    NoAppData,
    #[error("erro de E/S: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, ModelsError>;

/// Um modelo Whisper do catálogo.
#[derive(Debug, Clone, Copy)]
pub struct ModelInfo {
    /// Nome do arquivo no repositório ggerganov/whisper.cpp.
    pub file: &'static str,
    pub label: &'static str,
    pub approx_mb: u32,
    pub note: &'static str,
    /// Bom em GPU? (os grandes só valem a pena com CUDA)
    pub needs_gpu: bool,
}

pub const WHISPER_CATALOG: &[ModelInfo] = &[
    ModelInfo {
        file: "ggml-large-v3-turbo-q5_0.bin",
        label: "Large v3 Turbo (q5)",
        approx_mb: 574,
        note: "Recomendado com GPU: qualidade de large, 8× mais rápido",
        needs_gpu: true,
    },
    ModelInfo {
        file: "ggml-small.bin",
        label: "Small",
        approx_mb: 488,
        note: "Recomendado em CPU: leve e com boa qualidade em pt-BR",
        needs_gpu: false,
    },
    ModelInfo {
        file: "ggml-medium-q5_0.bin",
        label: "Medium (q5)",
        approx_mb: 539,
        note: "Meio-termo para CPU forte",
        needs_gpu: false,
    },
    ModelInfo {
        file: "ggml-large-v3-q5_0.bin",
        label: "Large v3 (q5)",
        approx_mb: 1080,
        note: "Máxima qualidade; mais lento — para reuniões importantes",
        needs_gpu: true,
    },
];

const HF_REPO: &str = "ggerganov/whisper.cpp";

pub fn catalog_entry(file: &str) -> Option<&'static ModelInfo> {
    WHISPER_CATALOG.iter().find(|m| m.file == file)
}

/// Pasta dos modelos: `%LOCALAPPDATA%\ISPer\models` (criada se não existir).
pub fn models_dir() -> Result<PathBuf> {
    let base = std::env::var("LOCALAPPDATA").map_err(|_| ModelsError::NoAppData)?;
    let dir = PathBuf::from(base).join("ISPer").join("models");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Caminho do modelo se estiver instalado na pasta padrão.
pub fn installed_path(file: &str) -> Option<PathBuf> {
    let p = models_dir().ok()?.join(file);
    p.exists().then_some(p)
}

/// Resolve qual arquivo de modelo usar, nesta ordem:
/// 1. env `ISPER_MODEL` (caminho completo);
/// 2. o preferido (nome do catálogo) na pasta padrão ou nas `extra_dirs`;
/// 3. o catálogo em ordem, respeitando GPU/CPU, na pasta padrão e nas
///    `extra_dirs` (as extras servem ao `models/` do repositório em dev).
pub fn resolve_whisper_model(
    preferred: Option<&str>,
    has_gpu: bool,
    extra_dirs: &[PathBuf],
) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ISPER_MODEL") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(d) = models_dir() {
        dirs.push(d);
    }
    dirs.extend(extra_dirs.iter().map(|d| d.join("models")));

    let find = |file: &str| dirs.iter().map(|d| d.join(file)).find(|p| p.exists());

    if let Some(pref) = preferred.filter(|p| !p.trim().is_empty()) {
        if let Some(p) = find(pref) {
            return Some(p);
        }
        tracing::warn!("modelo preferido {pref} não instalado — usando o melhor disponível");
    }
    // Preferência por adequação ao hardware: GPU → grandes primeiro; CPU → leves.
    let mut order: Vec<&ModelInfo> = WHISPER_CATALOG.iter().collect();
    order.sort_by_key(|m| if m.needs_gpu == has_gpu { 0 } else { 1 });
    order.iter().find_map(|m| find(m.file))
}

fn agent() -> ureq::Agent {
    // Sem timeout total: downloads de centenas de MB levam o tempo que levam.
    ureq::builder()
        .timeout_connect(std::time::Duration::from_secs(20))
        .timeout_read(std::time::Duration::from_secs(60))
        .build()
}

/// SHA-256 e tamanho publicados pelo Hugging Face para um arquivo LFS.
fn hf_expected(file: &str) -> Result<(Option<String>, Option<u64>)> {
    let url = format!("https://huggingface.co/api/models/{HF_REPO}/tree/main");
    let resp: serde_json::Value = agent()
        .get(&url)
        .call()
        .map_err(|e| ModelsError::Http(e.to_string()))?
        .into_json()
        .map_err(|e| ModelsError::Http(e.to_string()))?;
    let entry = resp
        .as_array()
        .and_then(|arr| arr.iter().find(|e| e["path"].as_str() == Some(file)));
    let Some(entry) = entry else {
        return Err(ModelsError::NotInCatalog(file.to_string()));
    };
    let sha = entry["lfs"]["oid"].as_str().map(str::to_string);
    let size = entry["lfs"]["size"].as_u64().or(entry["size"].as_u64());
    Ok((sha, size))
}

/// Baixa um modelo do catálogo para a pasta padrão, com progresso
/// `(bytes_baixados, bytes_totais)` e verificação de SHA-256.
pub fn download_whisper(file: &str, on_progress: &mut dyn FnMut(u64, u64)) -> Result<PathBuf> {
    if catalog_entry(file).is_none() {
        return Err(ModelsError::NotInCatalog(file.to_string()));
    }
    let (expected_sha, expected_size) = hf_expected(file)?;
    let url = format!("https://huggingface.co/{HF_REPO}/resolve/main/{file}");
    let dest = models_dir()?.join(file);
    download_asset(&url, &dest, expected_sha.as_deref(), expected_size, on_progress)?;
    Ok(dest)
}

/// Download genérico com progresso, escrita atômica (`.part` → final) e
/// verificação opcional de SHA-256.
pub fn download_asset(
    url: &str,
    dest: &Path,
    expected_sha256: Option<&str>,
    expected_size: Option<u64>,
    on_progress: &mut dyn FnMut(u64, u64),
) -> Result<()> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let resp = agent()
        .get(url)
        .call()
        .map_err(|e| ModelsError::Http(e.to_string()))?;
    let total = resp
        .header("content-length")
        .and_then(|v| v.parse::<u64>().ok())
        .or(expected_size)
        .unwrap_or(0);

    let part = dest.with_extension("part");
    let mut out = std::fs::File::create(&part)?;
    let mut reader = resp.into_reader();
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut done: u64 = 0;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        done += n as u64;
        on_progress(done, total);
    }
    out.flush()?;
    drop(out);

    if let Some(expected) = expected_sha256 {
        let got = format!("{:x}", hasher.finalize());
        if !got.eq_ignore_ascii_case(expected) {
            let _ = std::fs::remove_file(&part);
            return Err(ModelsError::Checksum {
                file: dest.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(),
                expected: expected.to_string(),
                got,
            });
        }
    }
    std::fs::rename(&part, dest)?;
    tracing::info!("baixado e verificado: {}", dest.display());
    Ok(())
}

/// Remove um modelo da pasta padrão (ignora se não existir).
pub fn remove(file: &str) -> Result<()> {
    let p = models_dir()?.join(file);
    if p.exists() {
        std::fs::remove_file(p)?;
    }
    Ok(())
}
