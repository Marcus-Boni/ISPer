//! Ditar uma tarefa no celular (Fase 10.6): uma frase curta, transcrita no
//! próprio aparelho com o Whisper que a transcrição local já baixou (9.4). O
//! áudio não sai do celular.
//!
//! O app grava em PCM 16 kHz mono (o `AudioRecord` do Android) e manda os
//! bytes; o motor fica carregado entre um ditado e outro, porque abrir o
//! modelo leva mais que transcrever uma frase.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use isper_core::{EngineOptions, WhisperEngine};

use crate::MobileError;

/// O motor aberto da última vez, com o caminho do modelo.
static ENGINE: Mutex<Option<(String, Arc<WhisperEngine>)>> = Mutex::new(None);

/// Menos que isso é um toque sem fala.
const MIN_SECS: f32 = 0.4;
/// Mais que isso não é uma tarefa: é uma reunião (use o gravador).
const MAX_SECS: f32 = 60.0;

fn engine(model_path: &str) -> Result<Arc<WhisperEngine>, MobileError> {
    let mut slot = ENGINE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((path, e)) = slot.as_ref()
        && path == model_path
    {
        return Ok(e.clone());
    }
    let e = Arc::new(WhisperEngine::new_with(
        Path::new(model_path),
        &EngineOptions { dtw: false },
    )?);
    *slot = Some((model_path.to_string(), e.clone()));
    Ok(e)
}

/// PCM 16 bits little-endian em amostras de -1 a 1.
fn samples(pcm16_le: &[u8]) -> Vec<f32> {
    pcm16_le
        .chunks_exact(2)
        .map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32_768.0)
        .collect()
}

/// Transcreve uma fala curta (PCM 16 kHz mono, 16 bits little-endian).
/// `lang` é `pt`, `en`… ou `auto`. Bloqueia: chame de uma thread de trabalho.
#[uniffi::export]
pub fn transcribe_dictation(
    pcm16_le: Vec<u8>,
    model_path: String,
    lang: String,
) -> Result<String, MobileError> {
    let audio = samples(&pcm16_le);
    let secs = audio.len() as f32 / 16_000.0;
    if secs < MIN_SECS {
        return Err(MobileError::Engine("fale um pouco mais".into()));
    }
    if secs > MAX_SECS {
        return Err(MobileError::Engine(
            "fala longa demais para uma tarefa (até 1 minuto)".into(),
        ));
    }
    let t = engine(&model_path)?.transcribe(&audio, &lang, None)?;
    let text = t.text.trim().to_string();
    if text.is_empty() {
        return Err(MobileError::Engine("não entendi; tente de novo".into()));
    }
    tracing::info!(secs, infer_secs = t.infer_secs, "ditado: {text}");
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_vira_amostras_e_fala_curta_e_recusada() {
        let pcm: Vec<u8> = [0i16, 16_384, -32_768]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        assert_eq!(samples(&pcm), [0.0, 0.5, -1.0]);
        let e =
            transcribe_dictation(vec![0; 3_200], "nao-existe.bin".into(), "pt".into()).unwrap_err();
        assert!(e.to_string().contains("fale um pouco mais"), "{e}");
    }
}
