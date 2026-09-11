//! Busca semântica (Fase 5): trechos de reuniões e ditados virando vetores
//! (`isper_llm::embeddings`), guardados no SQLite ao lado do texto
//! (`isper_core::store`), e a busca por similaridade que a Biblioteca oferece
//! ao lado da busca literal.
//!
//! Indexação: cada reunião salva (e cada ditado colado) é indexada em segundo
//! plano quando há provider configurado; "Indexar tudo" nas Configurações
//! cobre o histórico anterior e refaz o índice quando o modelo muda.

use crate::prelude::*;
use isper_core::embed;
use isper_core::store::{EmbeddingStats, MeetingStore, SemanticHit};
use isper_llm::embeddings::Embedder;

const SNIPPET_CHARS: usize = 220;

#[derive(Clone, Copy, serde::Serialize)]
pub(crate) struct IndexProgress {
    pub(crate) done: usize,
    pub(crate) total: usize,
}

#[derive(serde::Serialize)]
pub(crate) struct EmbeddingsStatus {
    provider: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
    /// Provider escolhido e (quando ele exige) chave presente.
    configured: bool,
    key_present: bool,
    /// `provider/modelo` gravado ao lado de cada vetor.
    model_id: Option<String>,
    stats: Option<EmbeddingStats>,
    indexing: Option<IndexProgress>,
}

#[derive(serde::Serialize)]
pub(crate) struct SemanticHitDto {
    kind: String,
    id: i64,
    /// Cosseno (0–1 na prática) do melhor trecho.
    score: f32,
    snippet: String,
    start_secs: Option<f32>,
    title: Option<String>,
    started_at: Option<String>,
    duration_secs: Option<f32>,
    /// Ditados: data/hora e texto completo.
    at: Option<String>,
    text: Option<String>,
}

#[derive(serde::Serialize)]
pub(crate) struct IndexReport {
    meetings: usize,
    dictations: usize,
    chunks: usize,
    skipped: usize,
    model_id: String,
}

fn embedder() -> isper_llm::Result<Box<dyn Embedder>> {
    isper_llm::embedder_from_settings(&isper_llm::load_settings().embeddings)
}

fn snippet(text: &str) -> String {
    if text.chars().count() <= SNIPPET_CHARS {
        return text.to_string();
    }
    let cut: String = text.chars().take(SNIPPET_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
}

/// Reunião → trechos do transcript (com instante) + resumo (sem instante).
fn meeting_chunks(store: &MeetingStore, meeting_id: i64) -> anyhow::Result<Vec<embed::Chunk>> {
    let detail = store
        .get_meeting(meeting_id)?
        .ok_or_else(|| anyhow::anyhow!("reunião {meeting_id} não encontrada"))?;
    let mut chunks = embed::chunk_meeting(&segment_refs(&detail));
    if let Some(summary) = detail.summary.as_deref().map(str::trim) {
        chunks.extend(
            embed::chunk_text(summary, embed::CHUNK_TARGET_CHARS)
                .into_iter()
                .map(|text| embed::Chunk {
                    start_secs: None,
                    text,
                }),
        );
    }
    Ok(chunks)
}

/// Indexa (ou reindexa) uma reunião. Devolve quantos trechos gravou.
fn index_meeting(
    store: &MeetingStore,
    emb: &dyn Embedder,
    meeting_id: i64,
) -> anyhow::Result<usize> {
    let chunks = meeting_chunks(store, meeting_id)?;
    if chunks.is_empty() {
        return Ok(0);
    }
    let texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();
    let vectors = emb.embed_documents(&texts)?;
    let rows: Vec<(Option<f32>, &str, &[f32])> = chunks
        .iter()
        .zip(&vectors)
        .map(|(c, v)| (c.start_secs, c.text.as_str(), v.as_slice()))
        .collect();
    store.replace_embeddings("meeting", meeting_id, &emb.id(), &rows)?;
    Ok(rows.len())
}

fn index_dictation(
    store: &MeetingStore,
    emb: &dyn Embedder,
    id: i64,
    text: &str,
) -> anyhow::Result<usize> {
    let pieces = embed::chunk_text(text, embed::CHUNK_TARGET_CHARS);
    if pieces.is_empty() {
        return Ok(0);
    }
    let texts: Vec<&str> = pieces.iter().map(String::as_str).collect();
    let vectors = emb.embed_documents(&texts)?;
    let rows: Vec<(Option<f32>, &str, &[f32])> = texts
        .iter()
        .zip(&vectors)
        .map(|(t, v)| (None, *t, v.as_slice()))
        .collect();
    store.replace_embeddings("dictation", id, &emb.id(), &rows)?;
    Ok(rows.len())
}

/// Reunião recém-salva (ou com resumo novo): indexa em segundo plano se houver
/// provider. Falha só vai ao log — a reunião já está salva.
pub(crate) fn index_meeting_background(app: &AppHandle, meeting_id: i64) {
    if !isper_llm::load_settings().embeddings.is_configured() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let outcome = embedder().map_err(anyhow::Error::from).and_then(|emb| {
            let store = open_store()?;
            index_meeting(&store, emb.as_ref(), meeting_id)
        });
        match outcome {
            Ok(n) => {
                tracing::info!(
                    meeting_id,
                    chunks = n,
                    "reunião indexada para busca semântica"
                );
                notify_status(&app);
            }
            Err(e) => tracing::warn!(meeting_id, "não consegui indexar a reunião: {e}"),
        }
    });
}

/// Ditado recém-colado: idem.
pub(crate) fn index_dictation_background(app: &AppHandle, id: i64, text: String) {
    if !isper_llm::load_settings().embeddings.is_configured() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let outcome = embedder().map_err(anyhow::Error::from).and_then(|emb| {
            let store = open_store()?;
            index_dictation(&store, emb.as_ref(), id, &text)
        });
        match outcome {
            Ok(_) => notify_status(&app),
            Err(e) => tracing::warn!(id, "não consegui indexar o ditado: {e}"),
        }
    });
}

/// Estado da busca semântica para Configurações e Biblioteca.
#[tauri::command]
pub(crate) fn embeddings_status(app: AppHandle) -> EmbeddingsStatus {
    let settings = isper_llm::load_settings().embeddings;
    let provider = settings.provider.trim().to_lowercase();
    let configured = settings.is_configured();
    let key_present = match provider.as_str() {
        "gemini" | "google" => isper_llm::get_api_key("gemini").ok().flatten().is_some(),
        "openai" | "ollama" => isper_llm::get_api_key(isper_llm::embeddings::OPENAI_COMPAT_KEY)
            .ok()
            .flatten()
            .is_some(),
        _ => false,
    };
    let model_id = settings.effective_model().map(|m| {
        format!(
            "{}/{m}",
            if provider == "google" {
                "gemini"
            } else if provider == "ollama" {
                "openai"
            } else {
                provider.as_str()
            }
        )
    });
    let stats = model_id
        .as_deref()
        .and_then(|id| open_store().ok()?.embeddings_stats(id).ok());
    let indexing = *app.state::<AppState>().indexing.lock_or_recover();
    EmbeddingsStatus {
        provider: configured.then_some(provider),
        model: settings.effective_model(),
        base_url: settings.base_url.clone(),
        configured,
        key_present,
        model_id,
        stats,
        indexing,
    }
}

/// Busca por similaridade em reuniões (`kind = "meeting"`) ou ditados
/// (`"dictation"`): a pergunta vira vetor no provider e é comparada com os
/// trechos guardados; volta o melhor trecho de cada item.
#[tauri::command]
pub(crate) async fn semantic_search(
    kind: String,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SemanticHitDto>, String> {
    let kind = if kind == "dictation" {
        "dictation"
    } else {
        "meeting"
    };
    let query = query.trim().to_string();
    if query.chars().count() < 2 {
        return Ok(Vec::new());
    }
    let limit = limit.unwrap_or(30).clamp(1, 100);
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<SemanticHitDto>, String> {
        let emb = embedder().map_err(|e| e.to_string())?;
        let store = open_store().map_err(|e| e.to_string())?;
        let vector = emb.embed_query(&query).map_err(|e| e.to_string())?;
        let hits: Vec<SemanticHit> = store
            .semantic_search(kind, &emb.id(), &vector, limit)
            .map_err(|e| e.to_string())?;
        let mut out = Vec::with_capacity(hits.len());
        for hit in hits {
            let dto = if kind == "meeting" {
                let Some(row) = store.meeting_row(hit.ref_id).map_err(|e| e.to_string())? else {
                    continue;
                };
                SemanticHitDto {
                    kind: kind.to_string(),
                    id: hit.ref_id,
                    score: hit.score,
                    snippet: snippet(&hit.text),
                    start_secs: hit.start_secs,
                    title: Some(row.title),
                    started_at: Some(row.started_at),
                    duration_secs: Some(row.duration_secs),
                    at: None,
                    text: None,
                }
            } else {
                let Some(row) = store.dictation_row(hit.ref_id).map_err(|e| e.to_string())? else {
                    continue;
                };
                SemanticHitDto {
                    kind: kind.to_string(),
                    id: hit.ref_id,
                    score: hit.score,
                    snippet: snippet(&hit.text),
                    start_secs: None,
                    title: None,
                    started_at: None,
                    duration_secs: None,
                    at: Some(row.at),
                    text: Some(row.text),
                }
            };
            out.push(dto);
        }
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Indexa o que falta (reuniões e ditados sem vetor para o modelo atual) e
/// descarta vetores de modelos antigos. Progresso em `isper-index-progress`.
#[tauri::command]
pub(crate) async fn index_all(app: AppHandle) -> Result<IndexReport, String> {
    {
        let state = app.state::<AppState>();
        let mut indexing = state.indexing.lock_or_recover();
        if indexing.is_some() {
            return Err("já há uma indexação em andamento".into());
        }
        *indexing = Some(IndexProgress { done: 0, total: 0 });
    }
    let app2 = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<IndexReport> {
        let emb = embedder()?;
        let model_id = emb.id();
        let store = open_store()?;
        let dropped = store.delete_embeddings_except_model(&model_id)?;
        if dropped > 0 {
            tracing::info!(dropped, "vetores de outros modelos descartados");
        }
        let done_meetings = store.embedded_ids("meeting", &model_id)?;
        let done_dictations = store.embedded_ids("dictation", &model_id)?;
        let meetings: Vec<i64> = store
            .meeting_ids()?
            .into_iter()
            .filter(|id| !done_meetings.contains(id))
            .collect();
        let dictations: Vec<(i64, String)> = store
            .dictation_texts()?
            .into_iter()
            .filter(|(id, _)| !done_dictations.contains(id))
            .collect();
        let total = meetings.len() + dictations.len();
        let skipped = done_meetings.len() + done_dictations.len();
        let mut report = IndexReport {
            meetings: 0,
            dictations: 0,
            chunks: 0,
            skipped,
            model_id: model_id.clone(),
        };
        let progress = |done: usize| {
            let p = IndexProgress { done, total };
            *app2.state::<AppState>().indexing.lock_or_recover() = Some(p);
            let _ = app2.emit("isper-index-progress", p);
        };
        progress(0);
        let mut done = 0usize;
        for id in meetings {
            match index_meeting(&store, emb.as_ref(), id) {
                Ok(n) => {
                    report.meetings += 1;
                    report.chunks += n;
                }
                Err(e) => tracing::warn!(meeting_id = id, "indexação falhou: {e}"),
            }
            done += 1;
            progress(done);
        }
        for (id, text) in dictations {
            match index_dictation(&store, emb.as_ref(), id, &text) {
                Ok(n) => {
                    report.dictations += 1;
                    report.chunks += n;
                }
                Err(e) => tracing::warn!(id, "indexação do ditado falhou: {e}"),
            }
            done += 1;
            progress(done);
        }
        tracing::info!(
            meetings = report.meetings,
            dictations = report.dictations,
            chunks = report.chunks,
            "índice semântico atualizado ({model_id})"
        );
        Ok(report)
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r.map_err(|e| e.to_string()));
    *app.state::<AppState>().indexing.lock_or_recover() = None;
    notify_status(&app);
    result
}

/// Chave do endpoint compatível com OpenAI (opcional; o Ollama local não usa).
#[tauri::command]
pub(crate) fn set_embeddings_key(app: AppHandle, key: String) -> Result<(), String> {
    let key = key.trim();
    let outcome = if key.is_empty() {
        isper_llm::delete_api_key(isper_llm::embeddings::OPENAI_COMPAT_KEY)
    } else {
        isper_llm::set_api_key(isper_llm::embeddings::OPENAI_COMPAT_KEY, key)
    };
    outcome.map_err(|e| e.to_string())?;
    notify_status(&app);
    Ok(())
}

/// Testa o provider de embeddings configurado: vetoriza uma frase e informa a dimensão.
#[tauri::command]
pub(crate) async fn test_embeddings() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let emb = embedder().map_err(|e| e.to_string())?;
        let started = Instant::now();
        let v = emb
            .embed_query("Teste de conexão da busca semântica do ISPer.")
            .map_err(|e| e.to_string())?;
        Ok(format!(
            "{} ({}): vetor de {} dimensões em {:.1} s",
            emb.name(),
            emb.model(),
            v.len(),
            started.elapsed().as_secs_f32()
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}
