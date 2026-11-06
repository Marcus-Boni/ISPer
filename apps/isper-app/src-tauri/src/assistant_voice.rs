//! Falar com o assistente no Hoje: o botão de microfone da barra de pergunta.
//!
//! Usa o gravador e o Whisper do ditado, sem um segundo caminho de áudio:
//! segurar o botão é falar, um toque é mãos-livres (o VAD encerra na pausa) e
//! Esc cancela. A gravação sai marcada com [`AudioRoute::Assistant`], e o laço do
//! gravador em `main.rs` a entrega a [`process`] em vez de colar.
//!
//! O texto ouvido vai para a caixa de pergunta, não direto para a IA: o
//! usuário confere (o Whisper erra nomes) e manda com Enter. Por isso não há
//! polimento por IA aqui, e a fala não entra no histórico de ditados (a
//! conversa é o registro). O dicionário pessoal e os comandos de voz valem
//! ("apagar isso" descarta), porque `transcribe` é o mesmo do ditado.

use crate::prelude::*;
use isper_core::RawAudio;

/// Evento com o andamento da escuta, para a tela Hoje.
const EVENT: &str = "isper-assist-voice";
/// Métrica local da transcrição (Diagnóstico).
pub(crate) const EVENT_ASSIST_VOICE: &str = "assistant_voice";
/// Menos que isso foi um clique, não uma fala.
const MIN_SPEECH_SECS: f32 = 0.4;

fn emit(app: &AppHandle, payload: serde_json::Value) {
    let _ = app.emit_to(MAIN, EVENT, payload);
}

/// O motor precisa estar pronto ANTES de gravar: não faz sentido o usuário
/// falar para nada.
fn engine_ready(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    if state.engine.lock_or_recover().is_some() {
        return Ok(());
    }
    let key = match *state.engine_status.lock_or_recover() {
        EngineStatus::Loading => "errors.model-loading",
        _ => "today.assist.mic-no-model",
    };
    Err(crate::i18n::tr(app, key))
}

/// Começa a ouvir para o assistente. Idempotente durante a própria escuta: o
/// toque rápido do botão chega depois de a gravação ter começado (no
/// `pointerdown`, para não perder a primeira sílaba) e só liga o mãos-livres.
#[tauri::command]
pub(crate) fn assistant_listen(app: AppHandle, handsfree: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut phase = state.phase.lock_or_recover();
    match *phase {
        Phase::Idle => {
            engine_ready(&app)?;
            *phase = Phase::Recording {
                started: Instant::now(),
                handsfree,
            };
            *state.route.lock_or_recover() = AudioRoute::Assistant { cancelled: false };
            drop(phase);
            // O canal do gravador é ordenado: o VAD vale para esta gravação.
            state.audio.start();
            if handsfree {
                state.audio.set_vad(true);
            }
            emit(&app, json!({ "stage": stage(handsfree) }));
            Ok(())
        }
        Phase::Recording {
            started,
            handsfree: was,
        } if *state.route.lock_or_recover() == AudioRoute::Assistant { cancelled: false } => {
            if handsfree && !was {
                *phase = Phase::Recording {
                    started,
                    handsfree: true,
                };
                drop(phase);
                state.audio.set_vad(true);
                emit(&app, json!({ "stage": stage(true) }));
            }
            Ok(())
        }
        // Um ditado ou uma captura de tarefas está com o microfone.
        _ => Err(crate::i18n::tr(&app, "today.assist.mic-busy")),
    }
}

fn stage(handsfree: bool) -> &'static str {
    if handsfree { "handsfree" } else { "listening" }
}

/// Para de ouvir: transcreve o que ouviu, ou descarta com `cancel` (Esc, saiu
/// da tela). Não faz nada se a escuta já acabou (o VAD pode ter chegado antes).
#[tauri::command]
pub(crate) fn assistant_listen_stop(app: AppHandle, cancel: bool) {
    let state = app.state::<AppState>();
    let mut phase = state.phase.lock_or_recover();
    if !matches!(*phase, Phase::Recording { .. }) {
        return;
    }
    {
        let mut route = state.route.lock_or_recover();
        if !route.is_assistant() {
            return;
        }
        if cancel {
            *route = AudioRoute::Assistant { cancelled: true };
        }
    }
    *phase = Phase::Processing;
    drop(phase);
    state.audio.stop();
}

/// A escuta do assistente terminou (botão solto, pausa no mãos-livres, Esc).
/// Chamada pelo laço do gravador; quem chama devolve a fase a `Idle`.
pub(crate) fn process(
    app: &AppHandle,
    result: Result<RawAudio, isper_core::IsperError>,
    cancelled: bool,
) {
    if cancelled {
        emit(app, payload(Ok(None)));
        return;
    }
    emit(app, json!({ "stage": "transcribing" }));
    let heard = result.map_err(anyhow::Error::from).and_then(|raw| {
        if raw.duration_secs() < MIN_SPEECH_SECS {
            anyhow::bail!(crate::i18n::tr(app, "errors.hold-mic"));
        }
        transcribe(app, raw, EVENT_ASSIST_VOICE)
    });
    let outcome = match heard {
        Ok(h) => Ok(h.map(|h| h.text)),
        Err(e) => {
            tracing::warn!("escuta do assistente falhou: {e}");
            Err(e.to_string())
        }
    };
    emit(app, payload(outcome));
}

/// O fim da escuta, como a tela recebe: o texto ouvido, nada (descartado) ou
/// o motivo do erro.
fn payload(outcome: Result<Option<String>, String>) -> serde_json::Value {
    match outcome {
        Ok(Some(text)) => json!({ "stage": "heard", "text": text }),
        Ok(None) => json!({ "stage": "discarded" }),
        Err(message) => json!({ "stage": "error", "message": message }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_fim_da_escuta_diz_o_que_a_tela_faz() {
        assert_eq!(
            payload(Ok(Some("o que tenho hoje".into()))),
            json!({ "stage": "heard", "text": "o que tenho hoje" })
        );
        assert_eq!(payload(Ok(None)), json!({ "stage": "discarded" }));
        assert_eq!(
            payload(Err("não entendi".into())),
            json!({ "stage": "error", "message": "não entendi" })
        );
    }

    #[test]
    fn a_rota_volta_ao_ditado_depois_de_cada_gravacao() {
        // O laço do gravador tira a rota com `take`: a próxima gravação, sem
        // ninguém marcar, é do atalho de ditado.
        let mut route = AudioRoute::Assistant { cancelled: true };
        assert_eq!(
            std::mem::take(&mut route),
            AudioRoute::Assistant { cancelled: true }
        );
        assert_eq!(route, AudioRoute::Dictation);
        assert!(AudioRoute::Assistant { cancelled: false }.is_assistant());
        assert!(!AudioRoute::Capture.is_assistant());
        assert!(!AudioRoute::Dictation.is_assistant());
    }
}
