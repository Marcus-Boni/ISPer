//! O motor Whisper: carrega o modelo uma vez, transcreve muitas vezes.
//!
//! `WhisperContext` (o modelo na memória) é caro de criar — no app final ele
//! vive o processo inteiro. Cada transcrição usa um `WhisperState` novo, que
//! é barato.

use std::path::Path;
use std::time::Instant;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::{IsperError, Result};

pub struct WhisperEngine {
    ctx: WhisperContext,
    /// Serializa inferências: ditado e blocos de reunião compartilham o
    /// mesmo modelo na GPU sem disputa.
    infer_lock: std::sync::Mutex<()>,
}

#[derive(Debug, Clone)]
pub struct TranscriptSegment {
    pub start_secs: f32,
    pub end_secs: f32,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Transcript {
    /// Texto completo, segmentos emendados.
    pub text: String,
    pub segments: Vec<TranscriptSegment>,
    /// Quanto tempo a inferência levou (para medir o fator de tempo real).
    pub infer_secs: f32,
    /// Duração do áudio transcrito.
    pub audio_secs: f32,
}

impl WhisperEngine {
    /// Carrega o modelo ggml. Operação cara (~1 s) — faça uma vez e reuse.
    pub fn new(model_path: &Path) -> Result<Self> {
        if !model_path.exists() {
            return Err(IsperError::Whisper(format!(
                "modelo não encontrado em {} — baixe de https://huggingface.co/ggerganov/whisper.cpp",
                model_path.display()
            )));
        }
        let path_str = model_path
            .to_str()
            .ok_or_else(|| IsperError::Whisper("caminho de modelo inválido".into()))?;
        let ctx = WhisperContext::new_with_params(path_str, WhisperContextParameters::default())
            .map_err(|e| IsperError::Whisper(e.to_string()))?;
        Ok(Self {
            ctx,
            infer_lock: std::sync::Mutex::new(()),
        })
    }

    /// Transcreve áudio já em 16 kHz mono f32 (use `RawAudio::into_whisper_input`).
    /// `lang` é o código do idioma ("pt", "en") ou "auto" para detecção.
    /// `initial_prompt` alimenta o dicionário pessoal: o Whisper tende a
    /// grafar corretamente termos que "acabou de ver" (nomes próprios, siglas).
    pub fn transcribe(
        &self,
        samples_16k: &[f32],
        lang: &str,
        initial_prompt: Option<&str>,
    ) -> Result<Transcript> {
        // Uma inferência por vez — ditado e reunião dividem a GPU em paz.
        let _guard = self.infer_lock.lock().unwrap();
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| IsperError::Whisper(e.to_string()))?;

        // Greedy é a estratégia rápida — suficiente para ditado. BeamSearch
        // fica para reuniões (Fase 4), onde qualidade importa mais que latência.
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(lang));
        if let Some(prompt) = initial_prompt {
            params.set_initial_prompt(prompt);
        }
        let threads = std::thread::available_parallelism()
            .map(|n| n.get() as i32)
            .unwrap_or(4);
        params.set_n_threads(threads.min(8));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        let started = Instant::now();
        state
            .full(params, samples_16k)
            .map_err(|e| IsperError::Whisper(e.to_string()))?;
        let infer_secs = started.elapsed().as_secs_f32();

        let mut segments = Vec::new();
        let n = state.full_n_segments();
        for i in 0..n {
            let Some(seg) = state.get_segment(i) else {
                break;
            };
            let text = seg
                .to_str_lossy()
                .map_err(|e| IsperError::Whisper(e.to_string()))?
                .into_owned();
            segments.push(TranscriptSegment {
                // Timestamps do whisper.cpp vêm em centissegundos.
                start_secs: seg.start_timestamp() as f32 / 100.0,
                end_secs: seg.end_timestamp() as f32 / 100.0,
                text: text.trim().to_string(),
            });
        }

        let text = segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();
        let audio_secs = samples_16k.len() as f32 / crate::WHISPER_SAMPLE_RATE as f32;

        Ok(Transcript {
            text,
            segments,
            infer_secs,
            audio_secs,
        })
    }
}
