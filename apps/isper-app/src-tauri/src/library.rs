//! Biblioteca: leitura, renomear, exportar (MD/SRT/DOCX) e regravação do Markdown a partir do banco.

use crate::prelude::*;
use isper_core::meeting::{self, SegmentRef};
use isper_core::store::{MeetingDetail, MeetingStore};

#[tauri::command]
pub(crate) fn list_meetings(
    query: Option<String>,
) -> Result<Vec<isper_core::store::MeetingRow>, String> {
    let store = open_store().map_err(|e| e.to_string())?;
    match query.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => store.search_meetings(q),
        None => store.list_meetings(),
    }
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn get_meeting(id: i64) -> Result<Option<isper_core::store::MeetingDetail>, String> {
    open_store()
        .map_err(|e| e.to_string())?
        .get_meeting(id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn rename_meeting(app: AppHandle, id: i64, title: String) -> Result<(), String> {
    if title.trim().is_empty() {
        return Err("título vazio".into());
    }
    let store = open_store().map_err(|e| e.to_string())?;
    store
        .rename_meeting(id, &title)
        .map_err(|e| e.to_string())?;
    rewrite_markdown(&store, id);
    notify_status(&app);
    Ok(())
}

/// Renomeia um falante nesta reunião ("Participante 1" → "Tatiana") em todos
/// os segmentos, e regrava o `.md` para acompanhar.
#[tauri::command]
pub(crate) fn rename_speaker(
    app: AppHandle,
    id: i64,
    from: String,
    to: String,
) -> Result<usize, String> {
    let to = to.trim();
    if to.is_empty() {
        return Err("nome vazio".into());
    }
    if to.chars().count() > 40 {
        return Err("nome longo demais (máx. 40 caracteres)".into());
    }
    let store = open_store().map_err(|e| e.to_string())?;
    let n = store
        .rename_speaker(id, &from, to)
        .map_err(|e| e.to_string())?;
    rewrite_markdown(&store, id);
    notify_status(&app);
    Ok(n)
}

/// Segmentos do banco como referências para os renderizadores do core.
pub(crate) fn segment_refs(detail: &MeetingDetail) -> Vec<SegmentRef<'_>> {
    detail
        .segments
        .iter()
        .map(|s| SegmentRef {
            speaker: &s.speaker,
            start_secs: s.start_secs,
            end_secs: s.end_secs,
            text: &s.text,
        })
        .collect()
}

/// Regrava o Markdown da reunião a partir do banco (fonte única): título,
/// falantes e resumo sempre iguais aos da Biblioteca. Falha só vai ao log —
/// o banco já está certo.
pub(crate) fn rewrite_markdown(store: &MeetingStore, id: i64) {
    let Ok(Some(detail)) = store.get_meeting(id) else {
        return;
    };
    let Some(path) = detail.meeting.md_path.as_deref() else {
        return;
    };
    let md = meeting::render_markdown(
        &detail.meeting.title,
        &detail.meeting.started_at,
        detail.meeting.duration_secs,
        &segment_refs(&detail),
        detail.summary.as_deref(),
        &detail.moments,
    );
    if let Err(e) = std::fs::write(path, md) {
        tracing::warn!("não consegui regravar {path}: {e}");
    }
}

/// Exporta a reunião ao lado do `.md` (SRT, DOCX ou MD) e abre o arquivo.
/// Devolve o caminho gravado.
#[tauri::command]
pub(crate) fn export_meeting(id: i64, format: String) -> Result<String, String> {
    let store = open_store().map_err(|e| e.to_string())?;
    let detail = store
        .get_meeting(id)
        .map_err(|e| e.to_string())?
        .ok_or("reunião não encontrada")?;
    let refs = segment_refs(&detail);
    let m = &detail.meeting;
    let (ext, bytes): (&str, Vec<u8>) = match format.to_lowercase().as_str() {
        "srt" => ("srt", isper_core::export::to_srt(&refs).into_bytes()),
        "docx" => (
            "docx",
            isper_core::export::to_docx(
                &m.title,
                &m.started_at,
                m.duration_secs,
                &refs,
                detail.summary.as_deref(),
                &detail.moments,
            ),
        ),
        "md" => (
            "md",
            meeting::render_markdown(
                &m.title,
                &m.started_at,
                m.duration_secs,
                &refs,
                detail.summary.as_deref(),
                &detail.moments,
            )
            .into_bytes(),
        ),
        other => return Err(format!("formato desconhecido: {other}")),
    };
    let base = match m.md_path.as_deref().map(Path::new) {
        Some(p) if p.parent().is_some() => p.with_extension(""),
        _ => meetings_dir()
            .map_err(|e| e.to_string())?
            .join(format!("reuniao-{id}")),
    };
    let out = base.with_extension(ext);
    std::fs::write(&out, bytes).map_err(|e| e.to_string())?;
    // Revela o arquivo no Explorer em vez de abri-lo: SRT/DOCX podem não ter
    // programa associado, e o diálogo "com qual app?" no meio do fluxo irrita.
    let _ = std::process::Command::new("explorer")
        .arg(format!("/select,{}", out.to_string_lossy()))
        .spawn();
    Ok(out.display().to_string())
}

/// Remove do histórico; o arquivo .md continua na pasta (decisão do usuário).
#[tauri::command]
pub(crate) fn delete_meeting(id: i64) -> Result<(), String> {
    open_store()
        .map_err(|e| e.to_string())?
        .delete_meeting(id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn open_meeting_file(id: i64) -> Result<(), String> {
    let store = open_store().map_err(|e| e.to_string())?;
    let detail = store
        .get_meeting(id)
        .map_err(|e| e.to_string())?
        .ok_or("reunião não encontrada")?;
    let path = detail
        .meeting
        .md_path
        .ok_or("esta reunião não tem arquivo .md registrado")?;
    if !std::path::Path::new(&path).exists() {
        return Err(format!("arquivo não encontrado: {path}"));
    }
    std::process::Command::new("cmd")
        .args(["/C", "start", "", &path])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub(crate) fn open_meetings_folder() -> Result<(), String> {
    let dir = meetings_dir().map_err(|e| e.to_string())?;
    std::process::Command::new("explorer")
        .arg(&dir)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub(crate) fn list_dictations(
    query: Option<String>,
) -> Result<Vec<isper_core::store::DictationRow>, String> {
    open_store()
        .map_err(|e| e.to_string())?
        .list_dictations(query.as_deref(), 300)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn delete_dictation(id: i64) -> Result<(), String> {
    open_store()
        .map_err(|e| e.to_string())?
        .delete_dictation(id)
        .map_err(|e| e.to_string())
}
