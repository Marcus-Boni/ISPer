//! Reuniões: iniciar/encerrar, salvar, título + resumo por IA, notificação e diarização em segundo plano.

use crate::prelude::*;
use isper_core::loopback::LoopbackSource;
use isper_core::meeting::{self, MeetingHandle, MeetingOptions, MeetingSegment, SegmentRef};

/// Depois de um ditado ou reunião: se houver reunião ativa, o overlay volta
/// a mostrar o estado dela; senão, esconde.
pub(crate) fn maybe_restore_overlay(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !matches!(*state.phase.lock().unwrap(), Phase::Idle) {
        return;
    }
    if state.meeting.lock().unwrap().is_some() {
        let _ = app.emit("isper-state", json!({"state": "meeting"}));
    } else if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.hide();
    }
}

/// Alterna a gravação de reunião (bandeja e tela Início). Encerrar devolve
/// `Ok` na hora — a transcrição segue em background e avisa pelo indicador;
/// falha ao INICIAR volta como erro para quem chamou mostrar.
pub(crate) fn toggle_meeting(app: &AppHandle) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    let mut slot = state.meeting.lock().unwrap();

    if let Some(handle) = slot.take() {
        drop(slot);
        *state.meeting_started.lock().unwrap() = None;
        set_meeting_text(app, &meeting_item_text(app, false));
        set_tray_recording(app, false);
        notify_status(app);
        let _ = app.emit("isper-state", json!({"state": "meeting-processing"}));
        show_overlay(app);
        let app = app.clone();
        std::thread::spawn(move || {
            match finish_meeting(&app, handle) {
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

    let engine = { state.engine.lock().unwrap().clone() };
    state.live.lock().unwrap().clear();
    state.moments.lock().unwrap().clear();
    // Cada fala transcrita durante a reunião vira um evento `isper-live` (Início
    // e indicador) e fica guardada para quem abrir a janela no meio.
    let live_app = app.clone();
    let on_segment: meeting::SegmentSink = Arc::new(move |seg: &MeetingSegment| {
        let item = LiveSegment {
            speaker: seg.speaker.label(),
            start_secs: seg.start_secs,
            end_secs: seg.end_secs,
            text: seg.text.clone(),
        };
        {
            let state = live_app.state::<AppState>();
            let mut live = state.live.lock().unwrap();
            live.push(item.clone());
            if live.len() > LIVE_KEEP {
                let excess = live.len() - LIVE_KEEP;
                live.drain(..excess);
            }
        }
        let _ = live_app.emit("isper-live", &item);
    });
    let opts = {
        let cfg = state.config.lock().unwrap();
        MeetingOptions {
            lang: cfg.lang.clone(),
            initial_prompt: cfg.initial_prompt(),
            source: LoopbackSource::parse(&cfg.meeting_source),
            input_device: cfg.input_device.clone(),
            on_segment: Some(on_segment),
            dictionary: cfg.dictionary.clone(),
        }
    };
    let started = engine
        .ok_or_else(|| anyhow::anyhow!("o modelo ainda está carregando — tente em instantes"))
        .and_then(|engine| meeting::start(engine, opts).map_err(anyhow::Error::from));
    match started {
        Ok(handle) => {
            let mut payload = json!({"state": "meeting"});
            if !handle.warnings.is_empty() {
                payload["message"] = json!(handle.warnings.join(" · "));
            }
            *state.meeting.lock().unwrap() = Some(handle);
            *state.meeting_started.lock().unwrap() = Some(Instant::now());
            set_meeting_text(app, &meeting_item_text(app, true));
            set_tray_recording(app, true);
            notify_status(app);
            let _ = app.emit("isper-state", payload);
            show_overlay(app);
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
pub(crate) fn finish_meeting(app: &AppHandle, handle: MeetingHandle) -> anyhow::Result<String> {
    let result = handle.stop()?;
    if result.segments.is_empty() {
        anyhow::bail!("nenhuma fala detectada na reunião");
    }
    let now = chrono::Local::now();
    let started_at = now.format("%d/%m/%Y %H:%M").to_string();
    let mut title = format!("Reunião — {started_at}");
    // Momentos marcados (★) durante a gravação: seção do Markdown (o resumo
    // por IA prioriza esses trechos), tabela no banco e chips na Biblioteca.
    let moments: Vec<f32> = {
        let mut taken = std::mem::take(&mut *app.state::<AppState>().moments.lock().unwrap());
        taken.sort_by(|a, b| a.total_cmp(b));
        taken
    };
    let md = meeting::to_markdown(&title, &started_at, &result, &moments);

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
    if !moments.is_empty() {
        if let Err(e) = store.save_moments(meeting_id, &moments) {
            tracing::warn!("não consegui guardar os momentos marcados: {e}");
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
                            "{}\n\n_Resumo gerado via {} ({})._",
                            summary.body.trim(),
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

    // O que fazer com a reunião pronta: notificar (padrão), abrir o .md ou nada.
    let after = app
        .state::<AppState>()
        .config
        .lock()
        .unwrap()
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

    // Fase 4: quem falou o quê — em segundo plano, se os modelos existirem.
    if isper_diarize::models_installed() && !result.others_audio_16k.is_empty() {
        diarize_in_background(app.clone(), meeting_id, result);
    }

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
    let line2 = format!(
        "{}{} · clique para abrir na Biblioteca",
        meeting::fmt_ts(duration_secs),
        if has_summary { " · resumo pronto" } else { "" }
    );
    let app2 = app.clone();
    let shown = notify::show(
        notify::Toast {
            title: "Reunião salva",
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

/// Roda a diarização numa thread, e ao terminar troca "Participantes" por
/// "Participante N" no banco, regrava o `.md` e avisa as janelas. Enquanto
/// roda, `diarizing` aponta para a reunião (o Início mostra um chip).
pub(crate) fn diarize_in_background(
    app: AppHandle,
    meeting_id: i64,
    mut result: meeting::MeetingResult,
) {
    {
        let state = app.state::<AppState>();
        *state.diarizing.lock().unwrap() = Some(meeting_id);
    }
    notify_status(&app);
    std::thread::spawn(move || {
        let started = Instant::now();
        let audio_secs =
            result.others_audio_16k.len() as f32 / isper_core::WHISPER_SAMPLE_RATE as f32;
        tracing::info!(
            meeting_id,
            audio_secs,
            "diarização iniciada em segundo plano"
        );
        let audio = result.others_audio_f32();
        let outcome = isper_diarize::diarize(&audio);
        drop(audio);
        match outcome {
            Ok(turns) => {
                let t: Vec<(f32, f32, usize)> =
                    turns.iter().map(|t| (t.start, t.end, t.speaker)).collect();
                result.apply_speaker_turns(&t);
                let labels: Vec<String> =
                    result.segments.iter().map(|s| s.speaker.label()).collect();
                let pairs: Vec<(f32, &str)> = result
                    .segments
                    .iter()
                    .zip(&labels)
                    .map(|(s, l)| (s.start_secs, l.as_str()))
                    .collect();
                let participants = result.distinct_participants();
                match open_store().and_then(|store| {
                    let n = store.relabel_segments(meeting_id, &pairs)?;
                    rewrite_markdown(&store, meeting_id);
                    let title = store
                        .get_meeting(meeting_id)?
                        .map(|d| d.meeting.title)
                        .unwrap_or_default();
                    Ok((n, title))
                }) {
                    Ok((n, title)) => {
                        tracing::info!(
                            meeting_id,
                            secs = started.elapsed().as_secs_f32(),
                            "{participants} participante(s) identificado(s); {n} falas rotuladas"
                        );
                        let notify_on =
                            app.state::<AppState>().config.lock().unwrap().after_meeting
                                == "notify";
                        if notify_on && n > 0 {
                            let line2 = format!(
                                "{} · clique para ver quem falou o quê",
                                if participants == 1 {
                                    "1 participante".to_string()
                                } else {
                                    format!("{participants} participantes")
                                }
                            );
                            let app2 = app.clone();
                            let _ = notify::show(
                                notify::Toast {
                                    title: "Falantes identificados",
                                    line1: &title,
                                    line2: Some(&line2),
                                    silent: true,
                                },
                                move || open_library_at(&app2, meeting_id),
                            );
                        }
                    }
                    Err(e) => tracing::warn!("diarização pronta, mas não consegui gravar: {e}"),
                }
            }
            Err(e) => tracing::warn!("diarização falhou (rótulos genéricos mantidos): {e}"),
        }
        {
            let state = app.state::<AppState>();
            let mut d = state.diarizing.lock().unwrap();
            if *d == Some(meeting_id) {
                *d = None;
            }
        }
        notify_status(&app);
    });
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
    let state = app.state::<AppState>();
    let live = state.live.lock().unwrap().clone();
    live
}

/// Marca o instante atual da reunião ("★"). Fica no estado até o fim da
/// gravação e vira seção do Markdown/DOCX, chips na Biblioteca e prioridade
/// no resumo por IA. Dois toques em menos de `MARK_DEBOUNCE` contam como um.
pub(crate) fn mark_moment(app: &AppHandle) -> anyhow::Result<f32> {
    let state = app.state::<AppState>();
    let started = *state.meeting_started.lock().unwrap();
    let Some(started) = started else {
        anyhow::bail!("nenhuma reunião em andamento");
    };
    let at = started.elapsed().as_secs_f32();
    {
        let mut moments = state.moments.lock().unwrap();
        if let Some(last) = moments.last().copied() {
            if at - last < MARK_DEBOUNCE.as_secs_f32() {
                return Ok(last);
            }
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
