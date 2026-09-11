//! Insights ao vivo: a cada N minutos de reunião, a janela recente do
//! transcript vai ao provider de IA ("o que ficou pendente? prometi algo?") e
//! o resultado aparece no card de reunião do Início (evento `isper-insights`).
//!
//! Opt-in (custa chamadas de API) e só com provider configurado. Uma rodada é
//! pulada quando quase não houve fala nova desde a anterior; "Atualizar agora"
//! força. Só o TEXTO viaja, como no resumo.

use crate::prelude::*;
use std::sync::mpsc;

/// Janela do transcript enviada em cada rodada.
const WINDOW_SECS: f32 = 15.0 * 60.0;
const WINDOW_MINUTES: u32 = 15;
/// Menos que isso de texto novo desde a última rodada = rodada pulada.
const MIN_NEW_CHARS: usize = 160;
/// Intervalos aceitos em Configurações (minutos).
pub(crate) const INSIGHTS_INTERVALS: [u32; 3] = [3, 5, 10];

#[derive(Clone, serde::Serialize)]
pub(crate) struct LiveInsights {
    pub(crate) text: String,
    /// Instante da reunião em que foi gerado.
    pub(crate) at_secs: f32,
    /// Hora do relógio (`HH:MM`), para o painel.
    pub(crate) updated_at: String,
    pub(crate) provider: String,
    pub(crate) model: String,
}

pub(crate) enum InsightsCmd {
    Now,
    Stop,
}

#[derive(Default)]
pub(crate) struct InsightsState {
    pub(crate) last: Option<LiveInsights>,
    pub(crate) running: bool,
    pub(crate) error: Option<String>,
    pub(crate) next_at: Option<Instant>,
    pub(crate) tx: Option<mpsc::Sender<InsightsCmd>>,
    /// Caracteres de fala já vistos na última rodada (pula rodadas sem novidade).
    pub(crate) seen_chars: usize,
}

/// O que o painel do Início renderiza.
#[derive(Clone, serde::Serialize)]
pub(crate) struct InsightsDto {
    pub(crate) enabled: bool,
    pub(crate) configured: bool,
    pub(crate) running: bool,
    pub(crate) interval_min: u32,
    pub(crate) next_in_secs: Option<u64>,
    pub(crate) last: Option<LiveInsights>,
    pub(crate) error: Option<String>,
}

pub(crate) fn insights_dto(app: &AppHandle) -> InsightsDto {
    let state = app.state::<AppState>();
    let (enabled, interval_min) = {
        let cfg = state.config.lock_or_recover();
        (cfg.live_insights, cfg.insights_interval_min)
    };
    let llm = isper_llm::load_settings();
    let configured = !llm.provider.trim().is_empty()
        && isper_llm::get_api_key(&llm.provider)
            .ok()
            .flatten()
            .is_some();
    let ins = state.insights.lock_or_recover();
    InsightsDto {
        enabled,
        configured,
        running: ins.running,
        interval_min,
        next_in_secs: ins
            .next_at
            .map(|t| t.saturating_duration_since(Instant::now()).as_secs()),
        last: ins.last.clone(),
        error: ins.error.clone(),
    }
}

fn emit(app: &AppHandle) {
    let _ = app.emit("isper-insights", insights_dto(app));
}

/// Começo de reunião: limpa a rodada anterior e, se ligado e configurado,
/// sobe o loop.
pub(crate) fn reset_insights(app: &AppHandle) {
    {
        let state = app.state::<AppState>();
        let mut ins = state.insights.lock_or_recover();
        if let Some(tx) = ins.tx.take() {
            let _ = tx.send(InsightsCmd::Stop);
        }
        *ins = InsightsState::default();
    }
    let configured = !isper_llm::load_settings().provider.trim().is_empty();
    if configured {
        ensure_loop(app);
    }
    emit(app);
}

/// Fim de reunião: encerra o loop (o último resultado fica para o painel até
/// a próxima reunião).
pub(crate) fn stop_insights_loop(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut ins = state.insights.lock_or_recover();
    if let Some(tx) = ins.tx.take() {
        let _ = tx.send(InsightsCmd::Stop);
    }
    ins.next_at = None;
    ins.running = false;
}

/// Sobe o loop se não houver um rodando e houver reunião em andamento.
fn ensure_loop(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.meeting_started.lock_or_recover().is_none() {
        return;
    }
    let mut ins = state.insights.lock_or_recover();
    if ins.tx.is_some() {
        return;
    }
    let (tx, rx) = mpsc::channel::<InsightsCmd>();
    ins.tx = Some(tx);
    drop(ins);

    let app = app.clone();
    std::thread::spawn(move || {
        loop {
            let interval = {
                let state = app.state::<AppState>();
                let cfg = state.config.lock_or_recover();
                Duration::from_secs(60 * u64::from(cfg.insights_interval_min.max(1)))
            };
            {
                let state = app.state::<AppState>();
                state.insights.lock_or_recover().next_at = Some(Instant::now() + interval);
            }
            emit(&app);
            let force = match rx.recv_timeout(interval) {
                Ok(InsightsCmd::Now) => true,
                Ok(InsightsCmd::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => false,
            };
            if app
                .state::<AppState>()
                .meeting_started
                .lock_or_recover()
                .is_none()
            {
                break;
            }
            // Com o recurso desligado o loop só atende "Atualizar agora" (rodada
            // avulsa) — as rodadas periódicas ficam para quem ligou.
            let enabled = app
                .state::<AppState>()
                .config
                .lock_or_recover()
                .live_insights;
            if !force && !enabled {
                continue;
            }
            generate(&app, force);
        }
        let state = app.state::<AppState>();
        let mut ins = state.insights.lock_or_recover();
        ins.tx = None;
        ins.next_at = None;
        ins.running = false;
        drop(ins);
        emit(&app);
    });
}

/// Uma rodada: janela recente → provider → estado + evento.
fn generate(app: &AppHandle, force: bool) {
    let state = app.state::<AppState>();
    let Some(started) = *state.meeting_started.lock_or_recover() else {
        return;
    };
    let elapsed = started.elapsed().as_secs_f32();
    let live = state.live.lock_or_recover().clone();
    let total_chars: usize = live.iter().map(|s| s.text.len()).sum();
    {
        let ins = state.insights.lock_or_recover();
        if !force && total_chars.saturating_sub(ins.seen_chars) < MIN_NEW_CHARS {
            tracing::debug!("insights: pouca fala nova — rodada pulada");
            return;
        }
    }
    if live.is_empty() {
        if force {
            let mut ins = state.insights.lock_or_recover();
            ins.error = Some("ainda não há fala transcrita nesta reunião".into());
            drop(ins);
            emit(app);
        }
        return;
    }
    let window: String = live
        .iter()
        .filter(|s| s.start_secs >= elapsed - WINDOW_SECS)
        .map(|s| {
            format!(
                "[{}] {}: {}",
                isper_core::meeting::fmt_ts(s.start_secs),
                s.speaker,
                s.text.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let settings = isper_llm::load_settings();
    let provider = match isper_llm::provider_from_settings(&settings) {
        Ok(p) => p,
        Err(e) => {
            let mut ins = state.insights.lock_or_recover();
            ins.error = Some(match e {
                isper_llm::LlmError::NotConfigured => {
                    "sem provider de IA — configure em Configurações → Inteligência".to_string()
                }
                other => other.to_string(),
            });
            drop(ins);
            emit(app);
            return;
        }
    };

    let previous = {
        let mut ins = state.insights.lock_or_recover();
        ins.running = true;
        ins.error = None;
        ins.last.as_ref().map(|l| l.text.clone())
    };
    emit(app);
    let started_at = Instant::now();
    let outcome = isper_llm::live_insights(
        provider.as_ref(),
        &isper_llm::InsightsInput {
            window: &window,
            previous: previous.as_deref(),
            elapsed: &isper_core::meeting::fmt_ts(elapsed),
            window_minutes: WINDOW_MINUTES,
        },
    );
    {
        let mut ins = state.insights.lock_or_recover();
        ins.running = false;
        match outcome {
            Ok(text) => {
                tracing::info!(
                    secs = started_at.elapsed().as_secs_f32(),
                    "insights ao vivo atualizados via {}",
                    provider.name()
                );
                ins.last = Some(LiveInsights {
                    text,
                    at_secs: elapsed,
                    updated_at: chrono::Local::now().format("%H:%M").to_string(),
                    provider: provider.name().to_string(),
                    model: provider.model().to_string(),
                });
                ins.seen_chars = total_chars;
            }
            Err(e) => {
                tracing::warn!("insights ao vivo falharam: {e}");
                ins.error = Some(e.to_string());
            }
        }
    }
    emit(app);
}

/// Estado do painel (o Início pede ao abrir e a cada `isper-status`).
#[tauri::command]
pub(crate) fn live_insights_state(app: AppHandle) -> InsightsDto {
    insights_dto(&app)
}

/// "Atualizar agora": força uma rodada (e sobe o loop se o usuário acabou de
/// ligar o recurso no meio da reunião).
#[tauri::command]
pub(crate) fn insights_now(app: AppHandle) -> Result<(), String> {
    if app
        .state::<AppState>()
        .meeting_started
        .lock_or_recover()
        .is_none()
    {
        return Err("nenhuma reunião em andamento".into());
    }
    ensure_loop(&app);
    let state = app.state::<AppState>();
    let ins = state.insights.lock_or_recover();
    match ins.tx.as_ref() {
        Some(tx) => tx
            .send(InsightsCmd::Now)
            .map_err(|_| "loop de insights parado".to_string()),
        None => Err("não consegui iniciar os insights".into()),
    }
}
