//! Reuniões: iniciar/encerrar, salvar, título + resumo por IA, notificação e diarização em segundo plano.

use crate::prelude::*;
use isper_core::loopback::LoopbackSource;
use isper_core::meeting::{self, MeetingHandle, MeetingOptions, MeetingSegment, SegmentRef};

/// Depois de um ditado ou reunião: se houver reunião ativa, o overlay volta
/// a mostrar o estado dela; se estiver fixo, volta ao repouso; senão, esconde.
pub(crate) fn maybe_restore_overlay(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !matches!(*state.phase.lock_or_recover(), Phase::Idle) {
        return;
    }
    if state.meeting.lock_or_recover().is_some() {
        let _ = app.emit("isper-state", json!({"state": "meeting"}));
    } else if overlay_pinned(app) {
        let _ = app.emit("isper-state", json!({"state": "idle"}));
        show_overlay(app);
    } else if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.hide();
    }
}

/// Alterna a gravação de reunião (bandeja e tela Início). Encerrar devolve
/// `Ok` na hora — a transcrição segue em background e avisa pelo indicador;
/// falha ao INICIAR volta como erro para quem chamou mostrar.
pub(crate) fn toggle_meeting(app: &AppHandle) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    let mut slot = state.meeting.lock_or_recover();

    if let Some(handle) = slot.take() {
        drop(slot);
        *state.meeting_started.lock_or_recover() = None;
        stop_insights_loop(app);
        stop_copilot_loop(app);
        // Lido AGORA, com a reunião que acabou ainda no ar. `finish_meeting`
        // roda numa thread e ainda espera o worker do Whisper terminar —
        // começar outra reunião nesse intervalo zeraria os cards, e a ata
        // desta aqui sairia sem as decisões que você validou.
        let decisions = confirmed_cards(app);
        on_meeting_stopped(app);
        set_meeting_text(app, &meeting_item_text(app, false));
        set_tray_recording(app, false);
        notify_status(app);
        let _ = app.emit("isper-state", json!({"state": "meeting-processing"}));
        show_overlay(app);
        let app = app.clone();
        std::thread::spawn(move || {
            match finish_meeting(&app, handle, decisions) {
                Ok(path) => {
                    tracing::info!("reunião salva em {path}");
                    let _ = app.emit("isper-state", json!({"state": "meeting-done"}));
                }
                Err(e) => {
                    tracing::warn!("reunião falhou: {e}");
                    let _ = app.emit_to(
                        "overlay",
                        "isper-state",
                        json!({"state": "error", "message": e.to_string()}),
                    );
                }
            }
            // A Biblioteca e o Início mostram a reunião nova / os totais.
            notify_status(&app);
            std::thread::sleep(Duration::from_millis(2500));
            maybe_restore_overlay(&app);
        });
        return Ok(());
    }
    drop(slot);

    let engine = { state.engine.lock_or_recover().clone() };
    state.live.lock_or_recover().clear();
    state.moments.lock_or_recover().clear();
    // Cada fala transcrita durante a reunião vira um evento `isper-live` (Início
    // e indicador) e fica guardada para quem abrir a janela no meio.
    let live_app = app.clone();
    let on_segment: meeting::SegmentSink = Arc::new(move |seg: &MeetingSegment| {
        let item = LiveSegment {
            speaker: seg.speaker.label(),
            start_secs: seg.start_secs,
            end_secs: seg.end_secs,
            text: seg.text.clone(),
            provisional: false,
        };
        {
            let state = live_app.state::<AppState>();
            let mut live = state.live.lock_or_recover();
            live.push(item.clone());
            if live.len() > LIVE_KEEP {
                let excess = live.len() - LIVE_KEEP;
                live.drain(..excess);
            }
        }
        let _ = live_app.emit("isper-live", &item);
        on_live_segment(
            &live_app,
            &item.speaker,
            item.end_secs - item.start_secs,
            &item.text,
        );
    });
    // Legenda provisória (buffer ainda aberto): só para a tela — não entra na
    // lista guardada; o bloco final chega em segundos e a substitui.
    let partial_app = app.clone();
    let on_partial: meeting::SegmentSink = Arc::new(move |seg: &MeetingSegment| {
        let _ = partial_app.emit(
            "isper-live",
            &LiveSegment {
                speaker: seg.speaker.label(),
                start_secs: seg.start_secs,
                end_secs: seg.end_secs,
                text: seg.text.clone(),
                provisional: true,
            },
        );
    });
    // Cada bloco que passa pelo Whisper vira uma métrica local (Diagnóstico).
    let on_block: meeting::BlockSink = Arc::new(|b: &meeting::BlockStats| {
        record_event(
            EVENT_MEETING_BLOCK,
            b.infer_secs.is_some(),
            b.infer_secs,
            Some(b.block_secs),
        );
    });
    let opts = {
        let cfg = state.config.lock_or_recover();
        MeetingOptions {
            lang: cfg.lang.clone(),
            initial_prompt: cfg.initial_prompt(),
            source: LoopbackSource::parse(&cfg.meeting_source),
            input_device: cfg.input_device.clone(),
            on_segment: Some(on_segment),
            on_partial: Some(on_partial),
            on_block: Some(on_block),
            dictionary: cfg.dictionary.clone(),
        }
    };
    let started = engine
        .ok_or_else(|| anyhow::anyhow!(crate::i18n::tr(app, "errors.model-loading")))
        .and_then(|engine| meeting::start(engine, opts).map_err(anyhow::Error::from));
    match started {
        Ok(handle) => {
            let mut payload = json!({"state": "meeting"});
            if !handle.warnings.is_empty() {
                payload["message"] = json!(handle.warnings.join(" · "));
            }
            *state.meeting.lock_or_recover() = Some(handle);
            *state.meeting_started.lock_or_recover() = Some(Instant::now());
            set_meeting_text(app, &meeting_item_text(app, true));
            set_tray_recording(app, true);
            notify_status(app);
            let _ = app.emit("isper-state", payload);
            show_overlay(app);
            // Insights ao vivo (se ligados): rodadas periódicas sobre o transcript.
            reset_insights(app);
            reset_copilot(app);
            Ok(())
        }
        Err(e) => {
            tracing::error!("não consegui iniciar a reunião: {e}");
            let _ = app.emit_to(
                "overlay",
                "isper-state",
                json!({"state": "error", "message": e.to_string()}),
            );
            show_overlay(app);
            let app2 = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(2500));
                maybe_restore_overlay(&app2);
            });
            Err(e)
        }
    }
}

/// Encerra a gravação, salva o Markdown em Documentos\ISPer\Reunioes e no
/// banco SQLite, gera título + resumo por IA (Fase 5, se configurado) e abre o
/// arquivo. A diarização (quem falou o quê) roda DEPOIS, em segundo plano:
/// na CPU ela leva ~40% da duração da reunião — bloquear o fim da reunião
/// por isso fazia ninguém esperar, e os rótulos ficavam genéricos para sempre.
pub(crate) fn finish_meeting(
    app: &AppHandle,
    handle: MeetingHandle,
    decisions: Vec<isper_llm::CopilotCard>,
) -> anyhow::Result<String> {
    let result = handle.stop()?;
    if result.segments.is_empty() {
        anyhow::bail!(crate::i18n::tr(app, "errors.no-speech-meeting"));
    }
    let now = chrono::Local::now();
    let started_at = now.format("%d/%m/%Y %H:%M").to_string();
    let mut title = crate::i18n::trv(
        app,
        "meeting.default-title",
        &[("date", started_at.clone())],
    );
    // Momentos marcados (★) durante a gravação: seção do Markdown (o resumo
    // por IA prioriza esses trechos), tabela no banco e chips na Biblioteca.
    let moments: Vec<f32> = {
        let mut taken = std::mem::take(&mut *app.state::<AppState>().moments.lock_or_recover());
        taken.sort_by(|a, b| a.total_cmp(b));
        taken
    };
    // `decisions` vem pronto de quem encerrou a reunião (ver toggle_meeting):
    // relê-lo aqui seria tarde demais.
    let decisions_md = isper_llm::render_decisions_markdown(&decisions);
    let mut md = meeting::to_markdown(&title, &started_at, &result, &moments);
    if let Some(d) = decisions_md.as_deref() {
        md.push_str("\n---\n\n");
        md.push_str(d);
    }

    // O transcript é salvo ANTES do resumo: se a API falhar, nada se perde.
    let docs = meetings_dir()?;
    let md_path = docs.join(format!("reuniao-{}.md", now.format("%Y%m%d-%H%M%S")));
    std::fs::write(&md_path, &md)?;

    let store = open_store()?;
    let meeting_id = store.save(
        &title,
        &started_at,
        &result,
        Some(&md_path.to_string_lossy()),
    )?;
    if !moments.is_empty()
        && let Err(e) = store.save_moments(meeting_id, &moments)
    {
        tracing::warn!("não consegui guardar os momentos marcados: {e}");
    }
    // Aba "Decisões" da Biblioteca: o Copilot é de memória e some com a
    // reunião, então o que você validou precisa virar linha no banco.
    if !decisions.is_empty() {
        let rows = stored_decisions(&decisions);
        if let Err(e) = store.save_decisions(meeting_id, &rows) {
            tracing::warn!("não consegui guardar as decisões do Copilot: {e}");
        } else {
            tracing::info!(n = rows.len(), "decisões do Copilot guardadas");
        }
    }

    // Fase 5: título + resumo por IA de nuvem, numa chamada — só o TEXTO do
    // transcript sai da máquina. Com resposta, o Markdown é regravado inteiro
    // (título novo + resumo) a partir da mesma fonte que a Biblioteca usa.
    let settings = isper_llm::load_settings();
    match isper_llm::provider_from_settings(&settings) {
        Ok(provider) => {
            let _ = app.emit("isper-state", json!({"state": "meeting-summary"}));
            match isper_llm::summarize_meeting_titled(provider.as_ref(), &md) {
                Ok(summary) => {
                    if let Some(t) = summary
                        .title
                        .as_deref()
                        .map(str::trim)
                        .filter(|t| !t.is_empty())
                    {
                        title = t.to_string();
                        let _ = store.rename_meeting(meeting_id, &title);
                    }
                    let _ = store.set_summary(meeting_id, summary.body.trim());
                    let labels: Vec<String> =
                        result.segments.iter().map(|s| s.speaker.label()).collect();
                    let refs: Vec<SegmentRef<'_>> = result
                        .segments
                        .iter()
                        .zip(&labels)
                        .map(|(s, l)| SegmentRef {
                            speaker: l,
                            start_secs: s.start_secs,
                            end_secs: s.end_secs,
                            text: &s.text,
                        })
                        .collect();
                    let full = meeting::render_markdown(
                        &title,
                        &started_at,
                        result.duration_secs,
                        &refs,
                        Some(&format!(
                            "{}{}\n\n_Resumo gerado via {} ({})._",
                            summary.body.trim(),
                            decisions_md
                                .as_deref()
                                .map(|d| format!("\n\n{d}"))
                                .unwrap_or_default(),
                            provider.name(),
                            provider.model()
                        )),
                        &moments,
                    );
                    if let Err(e) = std::fs::write(&md_path, full) {
                        tracing::warn!("não consegui regravar o Markdown com o resumo: {e}");
                    }
                    tracing::info!("resumo e título gerados via {}", provider.name());
                }
                Err(e) => tracing::warn!("resumo falhou (transcript preservado): {e}"),
            }
        }
        Err(isper_llm::LlmError::NotConfigured) => {
            tracing::info!("sem provider de IA configurado — reunião salva sem resumo");
        }
        Err(e) => tracing::warn!("resumo indisponível: {e}"),
    }

    // Busca semântica: transcript + resumo viram vetores em segundo plano
    // (só com provider de embeddings configurado).
    index_meeting_background(app, meeting_id);

    // O que fazer com a reunião pronta: notificar (padrão), abrir o .md ou nada.
    let after = app
        .state::<AppState>()
        .config
        .lock_or_recover()
        .after_meeting
        .clone();
    let has_summary = store
        .get_meeting(meeting_id)
        .ok()
        .flatten()
        .map(|d| d.summary.is_some())
        .unwrap_or(false);
    match after.as_str() {
        "open" => open_file(&md_path),
        "silent" => {}
        _ => notify_meeting_saved(
            app,
            meeting_id,
            &title,
            result.duration_secs,
            has_summary,
            &md_path,
        ),
    }

    // Passe final: a transcrição oficial, refeita sobre o áudio inteiro em
    // segundo plano (VAD, beam search, quem falou o quê). Quando termina,
    // substitui a transcrição ao vivo no banco e regrava o `.md`.
    final_pass::run_in_background(app.clone(), meeting_id, result);

    Ok(md_path.display().to_string())
}

/// Abre um arquivo no programa padrão do Windows.
pub(crate) fn open_file(path: &Path) {
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", &path.to_string_lossy()])
        .spawn();
}

/// Toast "Reunião salva" — clicar abre a Biblioteca já naquela reunião. Se o
/// Windows recusar a notificação, abre o arquivo (o usuário não fica sem nada).
pub(crate) fn notify_meeting_saved(
    app: &AppHandle,
    meeting_id: i64,
    title: &str,
    duration_secs: f32,
    has_summary: bool,
    md_path: &Path,
) {
    let summary = if has_summary {
        crate::i18n::tr(app, "notify.summary-ready")
    } else {
        String::new()
    };
    let line2 = crate::i18n::trv(
        app,
        "notify.meeting-saved-line2",
        &[
            ("duration", meeting::fmt_ts(duration_secs)),
            ("summary", summary),
        ],
    );
    let heading = crate::i18n::tr(app, "notify.meeting-saved");
    let app2 = app.clone();
    let shown = notify::show(
        notify::Toast {
            title: &heading,
            line1: title,
            line2: Some(&line2),
            silent: false,
        },
        move || open_library_at(&app2, meeting_id),
    );
    match shown {
        Ok(()) => tracing::info!(meeting_id, "notificação de reunião salva enviada"),
        Err(e) => {
            tracing::warn!("notificação indisponível ({e}) — abrindo o arquivo");
            open_file(md_path);
        }
    }
}

/// Inicia/encerra a reunião a partir do Início. Roda fora da thread principal:
/// abrir os dispositivos de áudio leva um instante e a UI não pode congelar.
#[tauri::command]
pub(crate) async fn toggle_meeting_cmd(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || toggle_meeting(&app).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

/// Falas já transcritas da reunião em andamento (para quem abre o Início no meio).
#[tauri::command]
pub(crate) fn live_transcript(app: AppHandle) -> Vec<LiveSegment> {
    // Edição 2024: os temporários da expressão final (State, MutexGuard) são
    // soltos antes das variáveis locais — o `let` intermediário de antes saiu.
    let state = app.state::<AppState>();
    state.live.lock_or_recover().clone()
}

/// Marca o instante atual da reunião ("★"). Fica no estado até o fim da
/// gravação e vira seção do Markdown/DOCX, chips na Biblioteca e prioridade
/// no resumo por IA. Dois toques em menos de `MARK_DEBOUNCE` contam como um.
pub(crate) fn mark_moment(app: &AppHandle) -> anyhow::Result<f32> {
    let state = app.state::<AppState>();
    let started = *state.meeting_started.lock_or_recover();
    let Some(started) = started else {
        anyhow::bail!(crate::i18n::tr(app, "errors.no-meeting"));
    };
    let at = started.elapsed().as_secs_f32();
    {
        let mut moments = state.moments.lock_or_recover();
        if let Some(last) = moments.last().copied()
            && at - last < MARK_DEBOUNCE.as_secs_f32()
        {
            return Ok(last);
        }
        moments.push(at);
    }
    tracing::info!(at_secs = at, "momento marcado");
    let _ = app.emit("isper-moment", json!({ "at_secs": at }));
    Ok(at)
}

#[tauri::command]
pub(crate) fn mark_moment_cmd(app: AppHandle) -> Result<f32, String> {
    mark_moment(&app).map_err(|e| e.to_string())
}
