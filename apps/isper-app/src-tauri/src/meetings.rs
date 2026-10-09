//! Reuniões: iniciar/encerrar, salvar, título + resumo por IA, notificação e diarização em segundo plano.

use crate::prelude::*;
use isper_core::loopback::LoopbackSource;
use isper_core::meeting::{self, MeetingHandle, MeetingOptions, MeetingSegment};

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
        // começar outra reunião nesse intervalo zeraria o Copilot, e a ata
        // desta aqui sairia sem as decisões que você validou e sem as notas.
        let copilot = copilot_wrap_up(app);
        on_meeting_stopped(app);
        set_meeting_text(app, &meeting_item_text(app, false));
        set_tray_recording(app, false);
        notify_status(app);
        let _ = app.emit("isper-state", json!({"state": "meeting-processing"}));
        show_overlay(app);
        let app = app.clone();
        std::thread::spawn(move || {
            match finish_meeting(&app, handle, copilot) {
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
    copilot: CopilotWrapUp,
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
    // `copilot` vem pronto de quem encerrou a reunião (ver toggle_meeting):
    // relê-lo aqui seria tarde demais. As notas, não: se o estado ainda é
    // desta reunião, vale o que o usuário escreveu até agora.
    let decisions_md = isper_llm::render_decisions_markdown(&copilot.decisions);
    let transcript = meeting::to_markdown(&title, &started_at, &result, &moments);
    // O que vai para o provedor do resumo leva as decisões, não as notas. Elas
    // são rascunho do usuário e só saem da máquina quando ele pede
    // ("Enriquecer com a reunião"); mandá-las a cada reunião mudaria, calado,
    // o que o ISPer promete sobre privacidade.
    let mut para_resumo = transcript.clone();
    meeting::append_copilot_sections(&mut para_resumo, decisions_md.as_deref(), None);
    let mut md = transcript;
    meeting::append_copilot_sections(
        &mut md,
        decisions_md.as_deref(),
        Some(&wrap_up_notes(app, &copilot)),
    );

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
    if !copilot.decisions.is_empty() {
        let rows = stored_decisions(&copilot.decisions);
        if let Err(e) = store.save_decisions(meeting_id, &rows) {
            tracing::warn!("não consegui guardar as decisões do Copilot: {e}");
        } else {
            tracing::info!(n = rows.len(), "decisões do Copilot guardadas");
        }
    }
    // As notas também: e daqui em diante o bloco do Copilot grava direto
    // nesta reunião, se o usuário continuar escrevendo.
    attach_saved_meeting(app, &copilot, &store, meeting_id);

    // Fase 10.3: o evento da agenda em que a gravação aconteceu dá o nome da
    // reunião (o assunto do convite é o nome que ela tem) e fica ligado a
    // ela. Com o assunto, a IA não troca o título, nem agora nem no passe
    // final.
    let ended = now.fixed_offset();
    let started = ended - chrono::Duration::milliseconds((result.duration_secs * 1000.0) as i64);
    let mut app_title = Some(title.clone());
    if let Some(event) = crate::agenda::event_for_recording(app, started, ended) {
        match store
            .set_meeting_event(meeting_id, &crate::agenda::stored_event(&event))
            .and_then(|()| store.rename_meeting(meeting_id, &event.subject))
        {
            Ok(()) => {
                tracing::info!(meeting_id, "reunião casada com o evento da agenda");
                title = event.subject.clone();
                app_title = None;
            }
            Err(e) => tracing::warn!(meeting_id, "não consegui casar a reunião com a agenda: {e}"),
        }
    }

    // Fase 5: título + resumo por IA de nuvem, numa chamada — só o TEXTO do
    // transcript (com as decisões validadas) sai da máquina. É o resumo
    // provisório, do texto ao vivo, para haver um na hora: o passe final o
    // refaz sobre a transcrição oficial (ver `final_pass::refresh_derived`).
    //
    // Ao mesmo tempo (Fase 10.3), as ações de "Eu" vão para a caixa de
    // entrada: o aviso de "reunião salva" já diz quantas.
    let lines = crate::meeting_inbox::lines_of(&result);
    let decision_rows = stored_decisions(&copilot.decisions);
    let day = now.date_naive();
    let actions_title = title.clone();
    let (summary_title, actions) = std::thread::scope(|scope| {
        let collecting = scope.spawn(|| {
            crate::meeting_inbox::collect(
                app,
                meeting_id,
                &actions_title,
                day,
                &lines,
                &decision_rows,
            )
        });
        let t = summarize_saved(
            &store,
            meeting_id,
            &para_resumo,
            app_title.as_deref(),
            || {
                let _ = app.emit("isper-state", json!({"state": "meeting-summary"}));
            },
        );
        (t, collecting.join().unwrap_or(0))
    });
    if let Some(t) = summary_title {
        title = t.clone();
        app_title = Some(t);
    }

    // Busca semântica: transcript + resumo viram vetores em segundo plano
    // (só com provider de embeddings configurado). Também provisórios: o
    // passe final descarta estes vetores e indexa o texto dele.
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
            actions,
            &md_path,
        ),
    }

    // Passe final: a transcrição oficial, refeita sobre o áudio inteiro em
    // segundo plano (VAD, beam search, quem falou o quê). Quando termina,
    // substitui a transcrição ao vivo no banco, regrava o `.md` e refaz o
    // resumo e a busca. O título vai junto: é o que o app pôs, e só ele pode
    // dar lugar ao título da IA.
    final_pass::run_in_background(app.clone(), meeting_id, result, app_title);

    Ok(md_path.display().to_string())
}

/// Título + resumo por IA de uma reunião já salva (Fase 5): só o texto sai
/// da máquina. Sem provedor configurado, ou se ele falhar, nada muda — a
/// transcrição já está salva.
///
/// Com resposta, o resumo vai para o banco e o Markdown é regravado inteiro
/// a partir dele — a fonte que a Biblioteca usa. Com `rename_from`, o título
/// também passa a ser o da IA, se o da reunião ainda for esse — o que o app
/// pôs; o que o usuário escolheu à mão nunca é trocado. `on_start` roda
/// quando há provedor, antes da chamada (a tela avisa que o resumo está
/// saindo). Devolve o título novo, quando ele mudou.
pub(crate) fn summarize_saved(
    store: &isper_core::store::MeetingStore,
    meeting_id: i64,
    text: &str,
    rename_from: Option<&str>,
    on_start: impl FnOnce(),
) -> Option<String> {
    let settings = isper_llm::load_settings();
    let provider = match isper_llm::provider_from_settings(&settings) {
        Ok(p) => p,
        Err(isper_llm::LlmError::NotConfigured) => {
            tracing::info!("sem provider de IA configurado — reunião salva sem resumo");
            return None;
        }
        Err(e) => {
            tracing::warn!("resumo indisponível: {e}");
            return None;
        }
    };
    on_start();
    summarize_with(store, provider.as_ref(), meeting_id, text, rename_from)
}

/// [`summarize_saved`] com o provedor já em mãos (os testes passam um falso).
fn summarize_with(
    store: &isper_core::store::MeetingStore,
    provider: &dyn isper_llm::LlmProvider,
    meeting_id: i64,
    text: &str,
    rename_from: Option<&str>,
) -> Option<String> {
    let summary = match isper_llm::summarize_meeting_titled(provider, text) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("resumo falhou (transcript preservado): {e}");
            return None;
        }
    };
    let mut renamed = None;
    if let Some(expected) = rename_from
        && let Some(t) = summary
            .title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
    {
        match store.rename_meeting_if(meeting_id, expected, t) {
            Ok(true) => renamed = Some(t.to_string()),
            Ok(false) => tracing::info!(meeting_id, "título escolhido pelo usuário mantido"),
            Err(e) => tracing::warn!(meeting_id, "não consegui trocar o título: {e}"),
        }
    }
    let _ = store.set_summary(meeting_id, summary.body.trim());
    // Pelo banco, e não montado aqui: enquanto o resumo saía, o usuário pode
    // ter mexido nas notas. A linha do provedor não fica guardada, então vai
    // junto só nesta gravação.
    let assinado = format!(
        "{}\n\n_Resumo gerado via {} ({})._",
        summary.body.trim(),
        provider.name(),
        provider.model()
    );
    rewrite_markdown_with_summary(store, meeting_id, Some(&assinado));
    tracing::info!("resumo e título gerados via {}", provider.name());
    renamed
}

/// Abre um arquivo no programa padrão do Windows.
pub(crate) fn open_file(path: &Path) {
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", &path.to_string_lossy()])
        .spawn();
}

/// Toast "Reunião salva" — clicar abre a Biblioteca já naquela reunião. Se o
/// Windows recusar a notificação, abre o arquivo (o usuário não fica sem nada).
#[allow(clippy::too_many_arguments)]
pub(crate) fn notify_meeting_saved(
    app: &AppHandle,
    meeting_id: i64,
    title: &str,
    duration_secs: f32,
    has_summary: bool,
    actions: usize,
    md_path: &Path,
) {
    let summary = if has_summary {
        crate::i18n::tr(app, "notify.summary-ready")
    } else {
        String::new()
    };
    let actions = if actions > 0 {
        crate::i18n::trv(app, "notify.actions-inbox", &[("n", actions.to_string())])
    } else {
        String::new()
    };
    let line2 = crate::i18n::trv(
        app,
        "notify.meeting-saved-line2",
        &[
            ("duration", meeting::fmt_ts(duration_secs)),
            ("summary", summary),
            ("actions", actions),
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

#[cfg(test)]
mod tests {
    use super::*;
    use isper_core::meeting::{ChannelAudio, MeetingResult, Speaker};
    use isper_core::store::{MeetingStore, StoredDecision};
    use isper_llm::testing::FakeProvider;

    const TITULO_PADRAO: &str = "Reunião de 27/09/2026 10:00";

    /// Uma reunião como o passe final a encontra: salva com o texto ao vivo,
    /// com título e resumo provisórios da IA, uma decisão validada e notas
    /// do usuário — e já com a transcrição final no lugar.
    fn reuniao_com_transcricao_final(nome: &str) -> (MeetingStore, i64, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("isper-resumo-final-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let md = dir.join("reuniao.md");
        let store = MeetingStore::open(&dir.join("isper.db")).unwrap();
        let ao_vivo = MeetingResult {
            segments: vec![MeetingSegment {
                speaker: Speaker::Others,
                start_secs: 0.0,
                end_secs: 3.0,
                text: "fechamos em quarenta mio".into(),
            }],
            duration_secs: 60.0,
            others_audio: ChannelAudio::empty(),
            me_audio: ChannelAudio::empty(),
            forced_cuts: 0,
        };
        let id = store
            .save(
                TITULO_PADRAO,
                "27/09/2026 10:00",
                &ao_vivo,
                Some(&md.to_string_lossy()),
            )
            .unwrap();
        let provisorio = FakeProvider::replying("TÍTULO: Provisório\n\n## Resumo\nDo ao vivo.");
        assert_eq!(
            summarize_with(&store, &provisorio, id, "ao vivo", Some(TITULO_PADRAO)).as_deref(),
            Some("Provisório")
        );
        store
            .save_decisions(
                id,
                &[StoredDecision {
                    kind: "decision".into(),
                    title: "Preço de 40 mil".into(),
                    description: "Aprovado pelo cliente".into(),
                    owner: None,
                    due_date: None,
                    urgency: "high".into(),
                    at_secs: 1.0,
                }],
            )
            .unwrap();
        store.set_notes(id, "- ligar para o jurídico").unwrap();
        store
            .replace_segments(
                id,
                &[(
                    "Participante 1".to_string(),
                    0.0,
                    3.0,
                    "Fechamos em quarenta mil.".to_string(),
                )],
            )
            .unwrap();
        (store, id, md)
    }

    fn detalhe(store: &MeetingStore, id: i64) -> isper_core::store::MeetingDetail {
        store.get_meeting(id).unwrap().expect("a reunião existe")
    }

    #[test]
    fn resumo_final_substitui_o_provisorio_com_as_decisoes_e_sem_as_notas() {
        let (store, id, md) = reuniao_com_transcricao_final("substitui");
        let ia = FakeProvider::replying("TÍTULO: Fechamento do contrato\n\n## Resumo\nDo final.");
        let texto = summary_input(&detalhe(&store, id));
        assert_eq!(
            summarize_with(&store, &ia, id, &texto, Some("Provisório")).as_deref(),
            Some("Fechamento do contrato")
        );

        // O que saiu da máquina: o texto final e a decisão validada; nem as
        // notas, nem o resumo do ao vivo.
        let (_, enviado) = ia.single_call();
        assert!(enviado.contains("Fechamos em quarenta mil."));
        assert!(!enviado.contains("quarenta mio"));
        assert!(enviado.contains("Preço de 40 mil"));
        assert!(!enviado.contains("ligar para o jurídico"));
        assert!(!enviado.contains("Do ao vivo."));

        let d = detalhe(&store, id);
        assert_eq!(d.meeting.title, "Fechamento do contrato");
        assert_eq!(d.summary.as_deref(), Some("## Resumo\nDo final."));
        // A ata acompanha: resumo novo assinado, e as notas continuam nela.
        let ata = std::fs::read_to_string(&md).unwrap();
        assert!(ata.contains("Do final."));
        assert!(ata.contains("_Resumo gerado via fake (fake-1)._"));
        assert!(!ata.contains("Do ao vivo."));
        assert!(ata.contains("ligar para o jurídico"));
    }

    #[test]
    fn resumo_final_nao_troca_o_titulo_que_o_usuario_escolheu() {
        let (store, id, md) = reuniao_com_transcricao_final("renomeada");
        store.rename_meeting(id, "Com o João").unwrap();
        let ia = FakeProvider::replying("TÍTULO: Fechamento do contrato\n\n## Resumo\nDo final.");
        let texto = summary_input(&detalhe(&store, id));
        assert_eq!(
            summarize_with(&store, &ia, id, &texto, Some("Provisório")),
            None
        );
        let d = detalhe(&store, id);
        assert_eq!(d.meeting.title, "Com o João");
        assert_eq!(d.summary.as_deref(), Some("## Resumo\nDo final."));
        assert!(
            std::fs::read_to_string(&md)
                .unwrap()
                .starts_with("# Com o João\n")
        );
    }

    #[test]
    fn falha_da_ia_no_resumo_final_mantem_o_provisorio() {
        let (store, id, _) = reuniao_com_transcricao_final("falha");
        let ia = FakeProvider::failing(|| isper_llm::LlmError::Http("status 429".into()));
        let texto = summary_input(&detalhe(&store, id));
        assert_eq!(
            summarize_with(&store, &ia, id, &texto, Some("Provisório")),
            None
        );
        let d = detalhe(&store, id);
        assert_eq!(d.meeting.title, "Provisório");
        assert_eq!(d.summary.as_deref(), Some("## Resumo\nDo ao vivo."));
        assert_eq!(d.segments[0].text, "Fechamos em quarenta mil.");
    }
}
