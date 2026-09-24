//! Biblioteca: leitura, renomear, exportar (MD/SRT/DOCX) e regravação do Markdown a partir do banco.

use crate::prelude::*;
use isper_core::meeting::{self, SegmentRef};
use isper_core::store::{MeetingDetail, MeetingRow, MeetingStore};

#[tauri::command]
pub(crate) fn list_meetings(query: Option<String>) -> Result<Vec<MeetingRow>, String> {
    let store = open_store().map_err(|e| e.to_string())?;
    let mut rows = match query.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => store.search_meetings(q),
        None => store.list_meetings(),
    }
    .map_err(|e| e.to_string())?;
    // Excluída há instantes (janela do Desfazer aberta): já não aparece.
    let hidden = crate::undo::hidden_meetings();
    rows.retain(|r| !hidden.contains(&r.id));
    Ok(rows)
}

#[tauri::command]
pub(crate) fn get_meeting(id: i64) -> Result<Option<MeetingDetail>, String> {
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

/// Uma regravação de `.md` por vez.
///
/// Várias partes do app regravam a ata a partir do banco — o fim da reunião
/// com o resumo, o passe final, renomear, e agora cada edição das notas do
/// Copilot. Cada uma lê o banco e escreve o arquivo; sem esta trava, uma
/// leitura antiga podia chegar ao disco depois de uma mais nova e desfazer
/// a edição de alguém.
static MARKDOWN_WRITE: Mutex<()> = Mutex::new(());

/// A ata inteira a partir do banco: falas, momentos, resumo e, no fim, as
/// seções do Copilot — decisões validadas e notas. Uma reunião importada de
/// um arquivo de áudio (Fase 9.0) ganha, no cabeçalho, a linha que diz de
/// qual arquivo ela veio.
///
/// `summary` troca o resumo guardado: o fim da reunião usa para acrescentar
/// a linha de qual provedor gerou o resumo, que não fica no banco.
pub(crate) fn meeting_markdown(detail: &MeetingDetail, summary: Option<&str>) -> String {
    let mut md = meeting::render_markdown(
        &detail.meeting.title,
        &detail.meeting.started_at,
        detail.meeting.duration_secs,
        &segment_refs(detail),
        summary.or(detail.summary.as_deref()),
        &detail.moments,
    );
    if let Some(source) = detail.meeting.source_name.as_deref() {
        meeting::insert_source_line(&mut md, source);
    }
    meeting::append_copilot_sections(
        &mut md,
        decisions_markdown(&detail.decisions).as_deref(),
        detail.notes.as_deref(),
    );
    md
}

/// Regrava o Markdown da reunião a partir do banco (fonte única): título,
/// falantes, resumo, decisões e notas sempre iguais aos da Biblioteca. Falha
/// só vai ao log — o banco já está certo.
pub(crate) fn rewrite_markdown(store: &MeetingStore, id: i64) {
    rewrite_markdown_with_summary(store, id, None);
}

/// [`rewrite_markdown`] com o resumo dado no lugar do guardado.
pub(crate) fn rewrite_markdown_with_summary(store: &MeetingStore, id: i64, summary: Option<&str>) {
    let _uma_por_vez = MARKDOWN_WRITE.lock_or_recover();
    let Ok(Some(detail)) = store.get_meeting(id) else {
        return;
    };
    let Some(path) = detail.meeting.md_path.as_deref() else {
        return;
    };
    if let Err(e) = std::fs::write(path, meeting_markdown(&detail, summary)) {
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
        "md" => ("md", meeting_markdown(&detail, None).into_bytes()),
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
pub(crate) fn delete_meeting(id: i64) -> crate::undo::Scheduled {
    crate::undo::schedule(crate::undo::Doomed::Meeting(id))
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
    if !Path::new(&path).exists() {
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
    let mut rows = open_store()
        .map_err(|e| e.to_string())?
        .list_dictations(query.as_deref(), 300)
        .map_err(|e| e.to_string())?;
    let hidden = crate::undo::hidden_dictations();
    rows.retain(|r| !hidden.contains(&r.id));
    Ok(rows)
}

#[tauri::command]
pub(crate) fn delete_dictation(id: i64) -> crate::undo::Scheduled {
    crate::undo::schedule(crate::undo::Doomed::Dictation(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use isper_core::store::{StoredDecision, StoredSegment};

    fn detalhe(summary: Option<&str>, notes: Option<&str>) -> MeetingDetail {
        MeetingDetail {
            meeting: MeetingRow {
                id: 1,
                title: "Negociação".into(),
                started_at: "23/09/2026 10:00".into(),
                duration_secs: 120.0,
                segments: 1,
                participants: 1,
                has_summary: summary.is_some(),
                md_path: None,
                moments: 0,
                decisions: 1,
                source_name: None,
            },
            summary: summary.map(Into::into),
            segments: vec![StoredSegment {
                speaker: "Participante 1".into(),
                start_secs: 4.0,
                end_secs: 9.0,
                text: "Fechamos em quarenta mil.".into(),
            }],
            moments: Vec::new(),
            decisions: vec![StoredDecision {
                kind: "decision".into(),
                title: "Preço de 40 mil".into(),
                description: "Aprovado pelo cliente".into(),
                owner: None,
                due_date: None,
                urgency: "high".into(),
                at_secs: 4.0,
            }],
            notes: notes.map(Into::into),
        }
    }

    #[test]
    fn ata_regravada_pelo_banco_mantem_decisoes_e_notas() {
        // Era o que se perdia: o passe final e o renomear regravam a ata pelo
        // banco, e a regravação não conhecia as decisões.
        let md = meeting_markdown(
            &detalhe(Some("## Resumo\nFechado."), Some("- mandar contrato")),
            None,
        );
        assert!(md.contains(meeting::COPILOT_DECISIONS_HEADING));
        assert!(md.contains("**Preço de 40 mil**"));
        assert!(md.contains(meeting::NOTES_HEADING));
        assert!(md.trim_end().ends_with("- mandar contrato"));

        // E o caminho de volta: reimportar dá o mesmo resumo e as mesmas notas.
        let m = isper_core::import::parse_markdown("r.md", &md).expect("importa");
        assert_eq!(m.summary.as_deref(), Some("## Resumo\nFechado."));
        assert_eq!(m.notes.as_deref(), Some("- mandar contrato"));
    }

    #[test]
    fn ata_regravada_de_um_audio_importado_mantem_a_origem() {
        // A mesma armadilha das decisões: toda regravação passa por aqui, e a
        // linha da origem precisa vir do banco para não sumir na primeira.
        let mut d = detalhe(None, None);
        d.meeting.source_name = Some("Com fornecedor.m4a".into());
        let md = meeting_markdown(&d, None);
        assert!(md.contains("> Importada do arquivo `Com fornecedor.m4a`."));
        let m = isper_core::import::parse_markdown("r.md", &md).expect("importa");
        assert_eq!(m.source_name.as_deref(), Some("Com fornecedor.m4a"));
        assert!(!meeting_markdown(&detalhe(None, None), None).contains("Importada do arquivo"));
    }

    #[test]
    fn resumo_dado_substitui_o_guardado_so_nesta_gravacao() {
        let d = detalhe(Some("## Resumo\nFechado."), None);
        let assinado = "## Resumo\nFechado.\n\n_Resumo gerado via groq (llama)._";
        let md = meeting_markdown(&d, Some(assinado));
        assert!(md.contains("_Resumo gerado via groq (llama)._"));
        assert!(!meeting_markdown(&d, None).contains("Resumo gerado via"));
    }
}
