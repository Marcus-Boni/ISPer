//! Ditado: máquina de estados do atalho, transcrição, polimento opcional e colagem.

use crate::prelude::*;
use isper_core::RawAudio;

pub(crate) fn on_pressed(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut phase = state.phase.lock().unwrap();
    match *phase {
        Phase::Idle => {
            *phase = Phase::Recording {
                started: Instant::now(),
                handsfree: false,
            };
            drop(phase);
            let _ = app.emit("isper-state", json!({"state": "recording"}));
            show_overlay(app);
            state.audio.start();
        }
        Phase::Recording { started, handsfree } => {
            // Segundo toque encerra o mãos-livres na hora. Só conta DEPOIS de
            // virar mãos-livres: o auto-repeat dispara Pressed repetido
            // enquanto a tecla está segurada no push-to-talk.
            if handsfree && started.elapsed() > Duration::from_millis(500) {
                *phase = Phase::Processing;
                drop(phase);
                state.audio.stop();
            }
        }
        Phase::Processing => {}
    }
}

pub(crate) fn on_released(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut phase = state.phase.lock().unwrap();
    if let Phase::Recording {
        started,
        handsfree: false,
    } = *phase
    {
        if started.elapsed() < TAP_THRESHOLD {
            *phase = Phase::Recording {
                started,
                handsfree: true,
            };
            drop(phase);
            state.audio.set_vad(true);
            let _ = app.emit("isper-state", json!({"state": "recording-handsfree"}));
        } else {
            *phase = Phase::Processing;
            drop(phase);
            state.audio.stop();
        }
    }
}

pub(crate) fn dictate(app: &AppHandle, raw: RawAudio) -> anyhow::Result<String> {
    if raw.duration_secs() < 0.4 {
        anyhow::bail!("segure o atalho enquanto fala");
    }
    let _ = app.emit("isper-state", json!({"state": "transcribing"}));

    let state = app.state::<AppState>();
    let engine = {
        let guard = state.engine.lock().unwrap();
        guard
            .clone()
            .ok_or_else(|| anyhow::anyhow!("o modelo ainda está carregando — tente em instantes"))?
    };
    let (lang, prompt) = {
        let cfg = state.config.lock().unwrap();
        (cfg.lang.clone(), cfg.initial_prompt())
    };

    let audio_secs = raw.duration_secs();
    let samples = raw.into_whisper_input()?;
    let t = engine.transcribe(&samples, &lang, prompt.as_deref())?;
    let raw_text = t.text.trim().to_string();
    if raw_text.is_empty() {
        anyhow::bail!("não entendi — tente de novo");
    }
    tracing::info!(audio_secs, infer_secs = t.infer_secs, "transcrito: {raw_text}");

    // Polimento opcional por IA (só o texto viaja). Qualquer falha cola o original.
    let text = polish_if_enabled(app, &raw_text);
    paste_text(&text)?;

    // Histórico de ditados (Fase 3) — falha aqui não pode travar o fluxo.
    if let Ok(store) = open_store() {
        let at = chrono::Local::now().format("%d/%m/%Y %H:%M:%S").to_string();
        let raw = (text != raw_text).then_some(raw_text.as_str());
        let _ = store.save_dictation(&at, &text, raw, audio_secs, t.infer_secs);
    }
    Ok(text)
}

/// Passa o ditado pelo provider de IA quando o polimento está ligado.
pub(crate) fn polish_if_enabled(app: &AppHandle, raw_text: &str) -> String {
    let (enabled, style) = {
        let state = app.state::<AppState>();
        let cfg = state.config.lock().unwrap();
        (cfg.polish, cfg.polish_style.clone())
    };
    if !enabled {
        return raw_text.to_string();
    }
    let settings = isper_llm::load_settings();
    match isper_llm::provider_from_settings(&settings) {
        Ok(provider) => {
            let _ = app.emit("isper-state", json!({"state": "polishing"}));
            let started = Instant::now();
            match isper_llm::polish_dictation(provider.as_ref(), raw_text, &style) {
                Ok(polished) => {
                    tracing::info!(secs = started.elapsed().as_secs_f32(), "ditado polido via {}", provider.name());
                    polished
                }
                Err(e) => {
                    tracing::warn!("polimento falhou (colando o original): {e}");
                    raw_text.to_string()
                }
            }
        }
        Err(isper_llm::LlmError::NotConfigured) => {
            tracing::info!("polimento ligado sem provider de IA — colando o original");
            raw_text.to_string()
        }
        Err(e) => {
            tracing::warn!("polimento indisponível ({e}) — colando o original");
            raw_text.to_string()
        }
    }
}

/// Cola `text` no app focado: salva o clipboard, injeta o texto, simula
/// Ctrl+V e restaura o clipboard anterior. Como o overlay não é focável,
/// o foco continua no app do usuário e o paste cai no lugar certo.
pub(crate) fn paste_text(text: &str) -> anyhow::Result<()> {
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};

    let mut clipboard = arboard::Clipboard::new()?;
    let previous = clipboard.get_text().ok();
    clipboard.set_text(text.to_string())?;
    std::thread::sleep(Duration::from_millis(60));

    let mut enigo = Enigo::new(&Settings::default())?;
    enigo.key(Key::Control, Direction::Press)?;
    enigo.key(Key::Unicode('v'), Direction::Click)?;
    enigo.key(Key::Control, Direction::Release)?;

    // Dá tempo do app alvo ler o clipboard antes de restaurá-lo.
    std::thread::sleep(Duration::from_millis(300));
    if let Some(old) = previous {
        let _ = clipboard.set_text(old);
    }
    Ok(())
}
