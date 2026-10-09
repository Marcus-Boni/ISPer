//! Captura de tarefas por voz (Fase 10.1, [ADR 0021]).
//!
//! O atalho de anotar (Ctrl+Alt+A) grava como o ditado: segurar é falar, um
//! toque é mãos-livres e o silêncio encerra. O Whisper transcreve, a LLM
//! separa as tarefas (`isper_llm::extract_tasks`), o parser resolve as datas
//! (`isper_assist::capture::draft`) e uma janela pequena, sempre no topo,
//! mostra os cartões para revisar: Enter salva, Esc descarta, e o atalho de
//! novo acrescenta mais cartões.
//!
//! A LLM tem um prazo ([`LLM_BUDGET`]). Sem provider configurado, com erro ou
//! depois do prazo, a fala inteira vira um cartão só, com as datas que o
//! parser achar: a captura nunca fica presa esperando a API.
//!
//! [ADR 0021]: ../../../../docs/adr/0021-assistente-pessoal-no-isper.md

use crate::prelude::*;
use isper_assist::capture::draft;
use isper_assist::{Actor, AssistStore, Clock, NewTask, SourceKind, SystemClock};
use isper_core::RawAudio;

/// Rótulo da janela dos cartões.
pub(crate) const LABEL: &str = "capture";
/// Evento com o estado da janela.
const EVENT: &str = "isper-capture";
/// Métricas locais: a transcrição da captura e a extração pela LLM.
pub(crate) const EVENT_CAPTURE: &str = "task_capture";
pub(crate) const EVENT_EXTRACT: &str = "task_extract";
/// Quanto a LLM pode demorar antes de a fala virar um cartão só.
pub(crate) const LLM_BUDGET: Duration = Duration::from_secs(8);

/// Em que pé está a captura.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Stage {
    /// Nada em andamento (janela escondida).
    #[default]
    Idle,
    /// Gravando, com o atalho segurado.
    Listening,
    /// Gravando em mãos-livres: um toque encerra.
    Handsfree,
    /// Transcrito; a LLM está separando as tarefas.
    Understanding,
    /// Cartões prontos para revisar.
    Review,
    /// A fala não tinha tarefa.
    Empty,
    /// Algo falhou (a mensagem diz o quê).
    Error,
}

/// Um cartão: uma tarefa ainda não salva.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Card {
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) notes: String,
    #[serde(default)]
    pub(crate) planned_on: Option<chrono::NaiveDate>,
    #[serde(default)]
    pub(crate) planned_time: Option<String>,
    #[serde(default)]
    pub(crate) due_on: Option<chrono::NaiveDate>,
}

impl From<NewTask> for Card {
    fn from(t: NewTask) -> Self {
        Self {
            title: t.title,
            notes: t.notes,
            planned_on: t.planned_on,
            planned_time: t.planned_time,
            due_on: t.due_on,
        }
    }
}

/// O que a janela mostra. Mora no estado do app: a página relê ao abrir.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub(crate) struct CaptureView {
    pub(crate) stage: Stage,
    /// Cartões acumulados desde a última vez que se salvou ou descartou.
    pub(crate) cards: Vec<Card>,
    /// A última fala transcrita.
    pub(crate) text: String,
    /// Quem separou as tarefas: "gemini · gemini-3.8-flash" ou "local".
    pub(crate) via: String,
    /// Mensagem do erro, ou o aviso de que a LLM ficou de fora.
    pub(crate) message: Option<String>,
}

fn set_view(app: &AppHandle, f: impl FnOnce(&mut CaptureView)) {
    let view = {
        let state = app.state::<AppState>();
        let mut v = state.capture_view.lock_or_recover();
        f(&mut v);
        v.clone()
    };
    let _ = app.emit_to(LABEL, EVENT, &view);
}

// --------------------------------------------------------------- atalho

pub(crate) fn on_pressed(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut phase = state.phase.lock_or_recover();
    match *phase {
        Phase::Idle => {
            *phase = Phase::Recording {
                started: Instant::now(),
                handsfree: false,
            };
            drop(phase);
            *state.capturing.lock_or_recover() = true;
            set_view(app, |v| {
                v.stage = Stage::Listening;
                v.message = None;
            });
            show_window(app);
            state.audio.start();
        }
        Phase::Recording { started, handsfree } => {
            // Como no ditado: o segundo toque encerra o mãos-livres, mas só
            // depois de virar mãos-livres (o auto-repeat da tecla segurada
            // também chega como Pressed).
            if handsfree
                && started.elapsed() > Duration::from_millis(500)
                && *state.capturing.lock_or_recover()
            {
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
    if !*state.capturing.lock_or_recover() {
        return;
    }
    let mut phase = state.phase.lock_or_recover();
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
            set_view(app, |v| v.stage = Stage::Handsfree);
        } else {
            *phase = Phase::Processing;
            drop(phase);
            state.audio.stop();
        }
    }
}

// --------------------------------------------------------------- pipeline

/// A gravação do atalho de anotar terminou: transcreve e separa as tarefas.
pub(crate) fn process(app: &AppHandle, result: Result<RawAudio, isper_core::IsperError>) {
    let heard = result
        .map_err(anyhow::Error::from)
        .and_then(|raw| transcribe(app, raw, EVENT_CAPTURE));
    match heard {
        Ok(Some(heard)) => {
            save_to_history(app, &heard.text, &heard);
            understand(app, &heard.text);
        }
        Ok(None) => {
            // "apagar isso": a fala sai, os cartões de antes ficam.
            set_view(app, |v| {
                v.stage = if v.cards.is_empty() {
                    Stage::Idle
                } else {
                    Stage::Review
                };
            });
            if app
                .state::<AppState>()
                .capture_view
                .lock_or_recover()
                .cards
                .is_empty()
            {
                hide_window(app);
            }
        }
        Err(e) => {
            tracing::warn!("captura falhou: {e}");
            set_view(app, |v| {
                v.stage = Stage::Error;
                v.message = Some(e.to_string());
            });
        }
    }
}

/// O texto vira cartões: LLM com prazo, ou um cartão só pelo parser.
fn understand(app: &AppHandle, text: &str) {
    set_view(app, |v| {
        v.stage = Stage::Understanding;
        v.text = text.to_string();
        v.message = None;
    });
    let today = SystemClock.today();
    let outcome = extract_within(text, LLM_BUDGET);
    let (cards, via, message) = match outcome {
        Extracted::Tasks { cards, via, secs } => {
            record_event(EVENT_EXTRACT, true, Some(secs), None);
            (cards, via, None)
        }
        Extracted::Nothing { via, secs } => {
            record_event(EVENT_EXTRACT, true, Some(secs), None);
            (Vec::new(), via, None)
        }
        Extracted::Fallback { why } => {
            if why.is_some() {
                record_event(EVENT_EXTRACT, false, None, None);
            }
            let one = draft(text, None, None, None, today);
            (vec![Card::from(one)], "local".to_string(), why)
        }
    };
    set_view(app, |v| {
        let empty = cards.is_empty();
        v.cards.extend(cards);
        v.via = via;
        v.message = message;
        v.stage = if empty && v.cards.is_empty() {
            Stage::Empty
        } else {
            Stage::Review
        };
    });
}

enum Extracted {
    Tasks {
        cards: Vec<Card>,
        via: String,
        secs: f32,
    },
    Nothing {
        via: String,
        secs: f32,
    },
    /// A LLM ficou de fora: `why` diz por quê (`None` = nenhuma configurada).
    Fallback {
        why: Option<String>,
    },
}

/// Chama a LLM numa thread e espera no máximo `budget`. Passado o prazo, a
/// thread termina sozinha e a resposta tardia é descartada.
fn extract_within(text: &str, budget: Duration) -> Extracted {
    let provider = match isper_llm::provider_from_settings(&isper_llm::load_settings()) {
        Ok(p) => p,
        Err(_) => return Extracted::Fallback { why: None },
    };
    let via = format!("{} · {}", provider.name(), provider.model());
    let (tx, rx) = std::sync::mpsc::channel();
    let speech = text.to_string();
    let started = Instant::now();
    std::thread::spawn(move || {
        let _ = tx.send(isper_llm::extract_tasks(provider.as_ref(), &speech));
    });
    let today = SystemClock.today();
    match rx.recv_timeout(budget) {
        Ok(Ok(tasks)) if tasks.is_empty() => Extracted::Nothing {
            via,
            secs: started.elapsed().as_secs_f32(),
        },
        Ok(Ok(tasks)) => Extracted::Tasks {
            cards: tasks
                .iter()
                .map(|t| {
                    Card::from(draft(
                        &t.title,
                        t.when.as_deref(),
                        t.due.as_deref(),
                        t.notes.as_deref(),
                        today,
                    ))
                })
                .collect(),
            via,
            secs: started.elapsed().as_secs_f32(),
        },
        Ok(Err(e)) => {
            tracing::warn!("extração de tarefas falhou ({via}): {e}");
            Extracted::Fallback {
                why: Some(e.to_string()),
            }
        }
        Err(_) => {
            tracing::warn!("extração de tarefas passou do prazo ({via})");
            Extracted::Fallback {
                why: Some(format!("{via} demorou mais de {} s", budget.as_secs())),
            }
        }
    }
}

// ----------------------------------------------------------------- janela

fn build_window(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    tauri::WebviewWindowBuilder::new(app, LABEL, tauri::WebviewUrl::App("capture.html".into()))
        .title(crate::i18n::tr(app, "window.capture"))
        .initialization_script(crate::ui::boot_script(&crate::ui::current(app)))
        .inner_size(480.0, 460.0)
        .min_inner_size(380.0, 300.0)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .center()
        .focused(true)
        .build()
}

/// Mostra a janela dos cartões (criada na primeira vez; depois só escondida,
/// para a próxima captura abrir na hora).
pub(crate) fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    // Criar janela na thread principal é proibido no Windows (ver `navigate`).
    let app = app.clone();
    std::thread::spawn(move || {
        if app.get_webview_window(LABEL).is_some() {
            return;
        }
        if let Err(e) = build_window(&app) {
            tracing::error!("não consegui abrir a janela de captura: {e}");
        }
    });
}

fn hide_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.hide();
    }
}

// --------------------------------------------------------------- comandos

/// O estado atual (a página chama ao abrir e ao voltar a aparecer).
#[tauri::command]
pub(crate) fn capture_state(app: AppHandle) -> CaptureView {
    app.state::<AppState>()
        .capture_view
        .lock_or_recover()
        .clone()
}

/// Salva os cartões revisados como tarefas e fecha a janela. Devolve quantas.
#[tauri::command]
pub(crate) fn capture_save(app: AppHandle, cards: Vec<Card>) -> Result<usize, String> {
    let text = app
        .state::<AppState>()
        .capture_view
        .lock_or_recover()
        .text
        .clone();
    let path = db_path().map_err(|e| e.to_string())?;
    drop(isper_core::store::MeetingStore::open(&path).map_err(|e| e.to_string())?);
    let store = AssistStore::open(&path).map_err(|e| e.to_string())?;
    let mut saved = 0;
    for card in cards {
        if card.title.trim().is_empty() {
            continue;
        }
        let task = NewTask {
            notes: card.notes,
            planned_on: card.planned_on,
            planned_time: card.planned_time.filter(|t| !t.trim().is_empty()),
            due_on: card.due_on,
            source_kind: SourceKind::Voice,
            source_ref: Some(json!({ "fala": text })),
            ..NewTask::titled(card.title)
        };
        store
            .create_task(task, Actor::User)
            .map_err(|e| e.to_string())?;
        saved += 1;
    }
    set_view(&app, |v| *v = CaptureView::default());
    hide_window(&app);
    let _ = app.emit(crate::today::TASKS_EVENT, ());
    Ok(saved)
}

/// Descarta os cartões e fecha a janela.
#[tauri::command]
pub(crate) fn capture_discard(app: AppHandle) {
    set_view(&app, |v| *v = CaptureView::default());
    hide_window(&app);
}

/// Separa as tarefas de um texto escrito (ou colado), pelo mesmo caminho da
/// fala: abre a janela com os cartões. `async`: abre janela e espera a LLM
/// fora da thread principal.
#[tauri::command]
pub(crate) async fn capture_text(app: AppHandle, text: String) -> Result<(), String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(crate::i18n::tr(&app, "errors.not-understood"));
    }
    show_window(&app);
    tauri::async_runtime::spawn_blocking(move || understand(&app, &text))
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cartao_vem_do_rascunho_sem_perder_nada() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 7).unwrap();
        let c = Card::from(draft(
            "Ligar pro João",
            Some("amanhã às 3"),
            None,
            Some("orçamento"),
            today,
        ));
        assert_eq!(c.title, "Ligar pro João");
        assert_eq!(c.planned_on, chrono::NaiveDate::from_ymd_opt(2026, 10, 8));
        assert_eq!(c.planned_time.as_deref(), Some("15:00"));
        assert_eq!(c.notes, "orçamento");
    }

    #[test]
    fn sem_a_llm_a_fala_inteira_vira_um_cartao_com_as_datas() {
        // O recuo da captura: o mesmo rascunho, com a fala inteira de título.
        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 7).unwrap();
        let one = Card::from(draft("amanhã revisar o contrato", None, None, None, today));
        assert_eq!(one.title, "Revisar o contrato");
        assert_eq!(one.planned_on, chrono::NaiveDate::from_ymd_opt(2026, 10, 8));
    }

    #[test]
    fn o_estado_serializa_com_nomes_estaveis() {
        let v = CaptureView {
            stage: Stage::Review,
            ..CaptureView::default()
        };
        let j = serde_json::to_value(&v).unwrap();
        assert_eq!(j["stage"], json!("review"));
        assert_eq!(j["cards"], json!([]));
    }
}
