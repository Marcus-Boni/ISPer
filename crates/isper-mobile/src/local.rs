//! A transcrição no próprio celular (Fase 9.4): a gravação vira ata sem PC.
//!
//! O passe final é o mesmo do PC (VAD, busca em feixe, falante por palavra),
//! com um modelo do tamanho do aparelho ([`device_plan`]). Ele guarda cada
//! janela transcrita num arquivo ao lado da gravação
//! (`<id>.transcricao.jsonl`): se o Android parar o trabalho no meio — a
//! tomada saiu, o sistema precisou da memória —, a próxima rodada continua dali
//! ([`isper_core::pipeline::run_resumable`]).
//!
//! A ata sai no mesmo formato da do PC e no mesmo lugar (`<id>.ata.md`), com
//! um `<id>.ata.json` que diz que ela foi feita no celular. Quando a ata do PC
//! chega pela sincronia, ela substitui esta e o `.ata.json` sai: o PC tem o
//! modelo maior, a GPU e a IA do resumo.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use isper_core::capture::{self, CaptureManifest, CaptureState};
use isper_core::context::MeetingContext;
use isper_core::meeting::{self, SegmentRef};
use isper_core::pipeline::{self, Diarizer, FinalConfig};
use isper_core::{EngineOptions, WhisperEngine};
use serde::{Deserialize, Serialize};

use crate::sync::{RemoteStage, minutes_path, remote_view, write_atomic};
use crate::{MobileDiarizer, MobileError, ProgressListener, Stage};

/// Memória total (como o Android informa) a partir da qual o celular usa o
/// modelo small e separa os falantes. Um aparelho "de 6 GB" informa ~5,3 GB.
const SMALL_FROM_MB: u64 = 5_000;
/// Abaixo disto o celular não transcreve: só o PC. Um aparelho "de 3 GB"
/// informa ~2,7 GB.
const ON_DEVICE_FROM_MB: u64 = 3_000;

/// O modelo small do catálogo do celular.
pub const SMALL_MODEL: &str = "ggml-small-q5_1.bin";
/// O modelo base do catálogo do celular.
pub const BASE_MODEL: &str = "ggml-base-q5_1.bin";

/// Como este aparelho transcreve, pelo tamanho da memória. É o ponto de
/// partida: o laboratório mede cada aparelho, e quem usa pode trocar.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DevicePlan {
    /// O aparelho transcreve sozinho. Falso: fraco demais, só o PC.
    pub on_device: bool,
    /// O modelo Whisper indicado (arquivo do catálogo do celular).
    pub model_file: String,
    /// Separar os falantes no próprio celular.
    pub diarize: bool,
    /// Por que, numa frase para a tela de Ajustes.
    pub reason: String,
}

/// O plano para um aparelho com `total_ram_mb` de memória (o `totalMem` do
/// Android, em MB).
///
/// - a partir de ~6 GB: small (181 MB) e falantes no celular. Num Snapdragon
///   855, o small levou 1,43× a duração do áudio, e a separação de falantes
///   ficou perto da do PC (DER 22,1% contra 20,6%);
/// - de ~3 a ~6 GB: base (57 MB), e os falantes ficam para o PC;
/// - abaixo disso: o celular só grava, e quem transcreve é o PC.
#[uniffi::export]
pub fn device_plan(total_ram_mb: u64) -> DevicePlan {
    if total_ram_mb >= SMALL_FROM_MB {
        DevicePlan {
            on_device: true,
            model_file: SMALL_MODEL.into(),
            diarize: true,
            reason: "memória de sobra: o modelo small e os falantes separados no celular".into(),
        }
    } else if total_ram_mb >= ON_DEVICE_FROM_MB {
        DevicePlan {
            on_device: true,
            model_file: BASE_MODEL.into(),
            diarize: false,
            reason: "memória justa: o modelo base, e o PC separa os falantes".into(),
        }
    } else {
        DevicePlan {
            on_device: false,
            model_file: BASE_MODEL.into(),
            diarize: false,
            reason: "memória pouca para transcrever aqui: quem transcreve é o PC".into(),
        }
    }
}

/// De onde veio a ata de uma gravação.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "snake_case")]
pub enum MinutesOrigin {
    /// Feita no próprio celular.
    Device,
    /// Veio do PC pareado.
    Pc,
}

/// `<id>.ata.json`: a ata ao lado foi feita no celular.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct LocalMeta {
    origem: MinutesOrigin,
    /// Arquivo do modelo usado.
    modelo: String,
    /// Falantes separados (0: sem separação).
    falantes: u32,
}

fn meta_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.ata.json"))
}

pub(crate) fn checkpoint_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.transcricao.jsonl"))
}

/// `<id>.transcricao.erro`: por que a transcrição no celular falhou. Enquanto
/// ele existir, a gravação sai da fila (senão um áudio corrompido seria
/// tentado de novo em toda rodada).
fn error_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.transcricao.erro"))
}

/// Por que a transcrição no celular de `id` falhou, se falhou.
pub(crate) fn local_error(dir: &Path, id: &str) -> Option<String> {
    std::fs::read_to_string(error_path(dir, id))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Anota que a transcrição no celular de `id` falhou, e por quê. A gravação
/// sai da fila até [`retry_local`].
#[uniffi::export]
pub fn mark_local_failed(dir: String, id: String, reason: String) -> Result<(), MobileError> {
    capture::check_id(&id)?;
    write_atomic(&error_path(Path::new(&dir), &id), reason.as_bytes())?;
    Ok(())
}

/// Devolve à fila uma gravação cuja transcrição no celular falhou. O que já
/// tinha sido transcrito continua valendo.
#[uniffi::export]
pub fn retry_local(dir: String, id: String) -> Result<(), MobileError> {
    capture::check_id(&id)?;
    match std::fs::remove_file(error_path(Path::new(&dir), &id)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

fn load_meta(dir: &Path, id: &str) -> Option<LocalMeta> {
    std::fs::read(meta_path(dir, id))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
}

/// De onde veio a ata de `id`, se ela existe.
pub(crate) fn minutes_origin(dir: &Path, id: &str) -> Option<MinutesOrigin> {
    if !minutes_path(dir, id).is_file() {
        return None;
    }
    // Sem o `.ata.json` do celular, a ata é a que o PC mandou.
    Some(load_meta(dir, id).map_or(MinutesOrigin::Pc, |m| m.origem))
}

/// A ata do PC chegou: a do celular deixa de valer (a sincronia chama isto
/// depois de gravar a do PC por cima).
pub(crate) fn pc_minutes_arrived(dir: &Path, id: &str) {
    let _ = std::fs::remove_file(meta_path(dir, id));
    let _ = std::fs::remove_file(checkpoint_path(dir, id));
}

/// Apaga o que a transcrição local deixou de uma gravação (ao apagá-la).
pub(crate) fn forget(dir: &Path, id: &str) {
    let _ = std::fs::remove_file(meta_path(dir, id));
    let _ = std::fs::remove_file(checkpoint_path(dir, id));
    let _ = std::fs::remove_file(error_path(dir, id));
}

/// Quanto de uma transcrição interrompida já foi feito, de 0 a 1.
pub(crate) fn local_progress(dir: &Path, id: &str) -> Option<f32> {
    let (done, total) = pipeline::resume_progress(&checkpoint_path(dir, id))?;
    (total > 0).then(|| done as f32 / total as f32)
}

/// As gravações que o celular ainda tem de transcrever, da mais nova para a
/// mais antiga: terminadas (ou recuperadas, ou importadas), sem ata, sem uma
/// falha anotada ([`mark_local_failed`]) e que o PC não recebeu inteiras. Quando o PC já tem a gravação (na fila, processando
/// ou pronta), a ata dele vem pela sincronia, e transcrever aqui seria gastar
/// bateria à toa. `active_id` é a que está gravando agora.
#[uniffi::export]
pub fn pending_local(dir: String, active_id: Option<String>) -> Result<Vec<String>, MobileError> {
    let dir = PathBuf::from(dir);
    let mut out: Vec<CaptureManifest> = capture::list(&dir)?
        .into_iter()
        .filter(|m| m.state != CaptureState::Recording)
        .filter(|m| active_id.as_deref() != Some(m.id.as_str()))
        .filter(|m| !minutes_path(&dir, &m.id).is_file())
        .filter(|m| !error_path(&dir, &m.id).exists())
        .filter(|m| {
            !matches!(
                remote_view(&dir, &m.id).stage,
                RemoteStage::Queued | RemoteStage::Processing | RemoteStage::Ready
            )
        })
        .collect();
    out.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(out.into_iter().map(|m| m.id).collect())
}

/// Os modelos e o idioma da transcrição local.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct LocalOptions {
    /// Caminho do modelo Whisper (`ggml-*.bin`).
    pub model_path: String,
    /// Caminho do modelo de VAD.
    pub vad_path: String,
    /// Idioma da fala: `pt`, `en`… ou `auto`.
    pub lang: String,
    /// Pasta com os modelos de diarização; `None` não separa os falantes.
    pub diarize_models_dir: Option<String>,
}

/// O que a transcrição local produziu.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct LocalMinutes {
    /// A gravação.
    pub recording_id: String,
    /// Título da ata.
    pub title: String,
    /// Onde ficou a ata (Markdown).
    pub minutes_path: String,
    /// Falantes separados (0: sem separação).
    pub speakers: u32,
    /// Duração do áudio, em segundos.
    pub audio_secs: f32,
    /// Quanto levou esta rodada, em segundos.
    pub total_secs: f32,
    /// Janelas aproveitadas de uma rodada interrompida (0: começou do zero).
    pub resumed_windows: u32,
}

/// Uma transcrição local que pode ser cancelada de outra thread.
#[derive(Debug, Default, uniffi::Object)]
pub struct LocalTranscription {
    cancel: AtomicBool,
}

#[uniffi::export]
impl LocalTranscription {
    /// Uma transcrição nova, ainda não iniciada.
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Pede para parar. O que já foi transcrito fica guardado, e a próxima
    /// rodada continua dali.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Transcreve a gravação `id` de `dir` e grava a ata ao lado. Bloqueia até
    /// terminar: chame de uma thread de trabalho.
    pub fn run(
        &self,
        dir: String,
        id: String,
        options: LocalOptions,
        listener: Arc<dyn ProgressListener>,
    ) -> Result<LocalMinutes, MobileError> {
        transcribe(&self.cancel, Path::new(&dir), &id, &options, &listener)
    }
}

fn transcribe(
    cancel: &AtomicBool,
    dir: &Path,
    id: &str,
    options: &LocalOptions,
    listener: &Arc<dyn ProgressListener>,
) -> Result<LocalMinutes, MobileError> {
    let started = Instant::now();
    let manifest = CaptureManifest::load(&CaptureManifest::path_in(dir, id))?;
    if minutes_origin(dir, id) == Some(MinutesOrigin::Pc) {
        return Err(MobileError::SupersededByPc);
    }

    let on_decode = |fraction: f32| {
        listener.on_progress(Stage::Decode, (fraction * 100.0).round() as u64, 100);
    };
    let decoded = isper_core::decode::decode_to_16k(
        &manifest.audio_path(dir),
        Some(&on_decode),
        Some(cancel),
    )?;

    let cfg = FinalConfig {
        name: "celular".into(),
        lang: options.lang.clone(),
        ..FinalConfig::meeting_final(PathBuf::from(&options.vad_path))
    };
    let engine = WhisperEngine::new_with(
        Path::new(&options.model_path),
        &EngineOptions {
            dtw: cfg.decode.token_timestamps,
        },
    )?;
    let diarizer = match &options.diarize_models_dir {
        Some(models) => {
            let (segmentation, embedding) = isper_diarize::model_paths_in(Path::new(models));
            if !segmentation.exists() || !embedding.exists() {
                return Err(MobileError::Model(
                    "modelos de diarização ausentes — baixe antes de transcrever".into(),
                ));
            }
            Some(MobileDiarizer {
                segmentation,
                embedding,
                opts: isper_diarize::DiarizeOptions::default(),
                listener: Arc::clone(listener),
            })
        }
        None => None,
    };

    let checkpoint = checkpoint_path(dir, id);
    let resumed = pipeline::resume_progress(&checkpoint).map_or(0, |(done, _)| done);
    let on_window = |done: usize, total: usize| {
        listener.on_progress(Stage::Transcribe, done as u64, total as u64);
    };
    let out = pipeline::run_resumable(
        &engine,
        &decoded.samples_16k,
        &cfg,
        &MeetingContext::default(),
        diarizer.as_ref().map(|d| d as &dyn Diarizer),
        Some(&on_window),
        Some(cancel),
        Some(&checkpoint),
    )?;

    let rows = out.speaker_rows();
    if rows.is_empty() {
        // Uma ata vazia não serve a ninguém: vira uma falha anotada, com o
        // motivo, e sai da fila.
        let _ = std::fs::remove_file(&checkpoint);
        return Err(MobileError::Audio("não achei fala nesta gravação".into()));
    }
    let refs: Vec<SegmentRef<'_>> = rows
        .iter()
        .map(|(speaker, start, end, text)| SegmentRef {
            speaker,
            start_secs: *start,
            end_secs: *end,
            text,
        })
        .collect();
    let when = display_time(&manifest.started_at);
    let title = format!("Reunião — {when}");
    let moments: Vec<f32> = manifest.moments.iter().map(|&m| m as f32).collect();
    let audio_secs = decoded.duration_secs();
    let mut md = meeting::render_markdown(&title, &when, audio_secs, &refs, None, &moments);
    if let Some(name) = &manifest.source_name {
        meeting::insert_source_line(&mut md, name);
    }
    insert_after_header(
        &mut md,
        &format!(
            "> Feita no celular, com o modelo {}.",
            model_label(&options.model_path)
        ),
    );

    let speakers = if out.speakers_reliable() {
        out.report
            .diarization
            .as_ref()
            .map_or(0, |d| d.speakers as u32)
    } else {
        0
    };
    // A ata do PC pode ter chegado enquanto esta era feita: ela vale mais.
    if minutes_origin(dir, id) == Some(MinutesOrigin::Pc) {
        let _ = std::fs::remove_file(&checkpoint);
        return Err(MobileError::SupersededByPc);
    }
    let meta = LocalMeta {
        origem: MinutesOrigin::Device,
        modelo: file_name(&options.model_path),
        falantes: speakers,
    };
    let meta_json =
        serde_json::to_vec_pretty(&meta).map_err(|e| MobileError::Engine(e.to_string()))?;
    write_atomic(&meta_path(dir, id), &meta_json)?;
    let minutes = minutes_path(dir, id);
    write_atomic(&minutes, md.as_bytes())?;
    let _ = std::fs::remove_file(&checkpoint);
    let _ = std::fs::remove_file(error_path(dir, id));
    tracing::info!(
        id,
        audio_secs,
        secs = started.elapsed().as_secs_f32(),
        resumed,
        speakers,
        "ata feita no celular"
    );

    Ok(LocalMinutes {
        recording_id: id.to_string(),
        title,
        minutes_path: minutes.display().to_string(),
        speakers,
        audio_secs,
        total_secs: started.elapsed().as_secs_f32(),
        resumed_windows: resumed as u32,
    })
}

/// `26/09/2026 19:26` a partir do RFC 3339 do manifesto, na hora de quem
/// gravou (o fuso vem junto na string). Se não der para ler, a string como
/// veio.
fn display_time(rfc3339: &str) -> String {
    let b = rfc3339.as_bytes();
    let digits = |r: std::ops::Range<usize>| {
        b.get(r.clone())
            .is_some_and(|s| s.iter().all(u8::is_ascii_digit))
    };
    let ok = b.len() >= 16
        && digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && (b[10] == b'T' || b[10] == b' ')
        && digits(11..13)
        && b[13] == b':'
        && digits(14..16);
    if !ok {
        return rfc3339.to_string();
    }
    format!(
        "{}/{}/{} {}",
        &rfc3339[8..10],
        &rfc3339[5..7],
        &rfc3339[0..4],
        &rfc3339[11..16]
    )
}

/// Põe `line` logo abaixo das linhas de cabeçalho ("> …") do começo da ata.
fn insert_after_header(md: &mut String, line: &str) {
    let mut pos = 0;
    let mut seen_quote = false;
    for l in md.split_inclusive('\n') {
        if l.starts_with('>') {
            seen_quote = true;
        } else if seen_quote {
            break;
        }
        pos += l.len();
    }
    if !seen_quote {
        pos = md.find('\n').map_or(md.len(), |n| n + 1);
    }
    md.insert_str(pos, &format!("{line}\n"));
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}

/// O nome do modelo para a ata ("Small (q5)"), pelo catálogo do celular.
fn model_label(path: &str) -> String {
    let file = file_name(path);
    isper_models::MOBILE_CATALOG
        .iter()
        .find(|m| m.file == file)
        .map_or(file, |m| m.label.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plano_pela_memoria() {
        let s21 = device_plan(5_289);
        assert!(s21.on_device && s21.diarize);
        assert_eq!(s21.model_file, SMALL_MODEL);

        let tab_a9 = device_plan(3_600);
        assert!(tab_a9.on_device && !tab_a9.diarize);
        assert_eq!(tab_a9.model_file, BASE_MODEL);

        let fraco = device_plan(2_700);
        assert!(!fraco.on_device);
        // Os modelos do plano existem no catálogo do celular.
        for m in [SMALL_MODEL, BASE_MODEL] {
            assert!(
                isper_models::MOBILE_CATALOG.iter().any(|c| c.file == m),
                "{m}"
            );
        }
    }

    #[test]
    fn hora_de_quem_gravou() {
        assert_eq!(
            display_time("2026-09-26T19:26:46-03:00"),
            "26/09/2026 19:26"
        );
        assert_eq!(display_time("2026-09-26T22:26:46Z"), "26/09/2026 22:26");
        assert_eq!(display_time("ontem"), "ontem");
    }

    #[test]
    fn linha_entra_logo_depois_do_cabecalho() {
        let mut md =
            "# Título\n\n> Transcrito em x.\n> Lembrete.\n\n**[00:01] Participantes:** oi\n"
                .to_string();
        insert_after_header(&mut md, "> Feita no celular.");
        assert_eq!(
            md,
            "# Título\n\n> Transcrito em x.\n> Lembrete.\n> Feita no celular.\n\n**[00:01] Participantes:** oi\n"
        );
    }

    #[test]
    fn rotulo_do_modelo_pelo_catalogo() {
        assert_eq!(
            model_label("/data/models/ggml-small-q5_1.bin"),
            "Small (q5)"
        );
        assert_eq!(model_label("/x/outro.bin"), "outro.bin");
    }

    fn dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("isper-mobile-local-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn gravar(dir: &Path, id: &str, pcm_16k: &[f32]) {
        let r = crate::Recorder::start(
            dir.display().to_string(),
            id.into(),
            "2026-09-26T10:00:00-03:00".into(),
            16_000,
        )
        .unwrap();
        let bytes: Vec<u8> = pcm_16k
            .iter()
            .flat_map(|s| ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
            .collect();
        r.write(bytes).unwrap();
        r.finish().unwrap();
    }

    #[test]
    fn pendentes_e_origem_da_ata() {
        let d = dir("pendentes");
        let id = "20260926-100000";
        gravar(&d, id, &vec![0.0; 16_000]);
        let ds = d.display().to_string();

        assert_eq!(
            pending_local(ds.clone(), None).unwrap(),
            vec![id.to_string()]
        );
        assert!(
            pending_local(ds.clone(), Some(id.into()))
                .unwrap()
                .is_empty(),
            "a que está gravando fica de fora"
        );

        // O PC já tem a gravação inteira: a ata vem de lá.
        std::fs::write(d.join(format!("{id}.sync.json")), r#"{"stage":"queued"}"#).unwrap();
        assert!(pending_local(ds.clone(), None).unwrap().is_empty());
        std::fs::remove_file(d.join(format!("{id}.sync.json"))).unwrap();

        // Uma ata feita no celular.
        std::fs::write(minutes_path(&d, id), "# Reunião\n").unwrap();
        let meta = LocalMeta {
            origem: MinutesOrigin::Device,
            modelo: SMALL_MODEL.into(),
            falantes: 2,
        };
        std::fs::write(meta_path(&d, id), serde_json::to_vec(&meta).unwrap()).unwrap();
        assert_eq!(minutes_origin(&d, id), Some(MinutesOrigin::Device));
        assert!(
            pending_local(ds.clone(), None).unwrap().is_empty(),
            "já tem ata"
        );
        let listed = crate::list_recordings(ds.clone(), None).unwrap();
        assert_eq!(
            listed.recordings[0].minutes_origin,
            Some(MinutesOrigin::Device)
        );

        // A do PC chega por cima e passa a valer.
        std::fs::write(minutes_path(&d, id), "# Reunião do PC\n").unwrap();
        pc_minutes_arrived(&d, id);
        assert_eq!(minutes_origin(&d, id), Some(MinutesOrigin::Pc));
        assert!(!meta_path(&d, id).exists());

        // Uma falha anotada tira a gravação da fila até "Tentar de novo".
        std::fs::remove_file(minutes_path(&d, id)).unwrap();
        assert_eq!(
            pending_local(ds.clone(), None).unwrap(),
            vec![id.to_string()]
        );
        mark_local_failed(ds.clone(), id.into(), "áudio ilegível".into()).unwrap();
        assert!(pending_local(ds.clone(), None).unwrap().is_empty());
        assert_eq!(local_error(&d, id).as_deref(), Some("áudio ilegível"));
        let listed = crate::list_recordings(ds.clone(), None).unwrap();
        assert_eq!(
            listed.recordings[0].local_error.as_deref(),
            Some("áudio ilegível")
        );
        retry_local(ds.clone(), id.into()).unwrap();
        retry_local(ds.clone(), id.into()).unwrap(); // de novo não é erro
        assert_eq!(
            pending_local(ds.clone(), None).unwrap(),
            vec![id.to_string()]
        );
        assert!(mark_local_failed(ds.clone(), "../fora".into(), "x".into()).is_err());

        // Apagar a gravação leva tudo junto.
        crate::delete_recording(ds.clone(), id.into()).unwrap();
        assert!(!minutes_path(&d, id).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    struct Silent;
    impl ProgressListener for Silent {
        fn on_progress(&self, _: Stage, _: u64, _: u64) {}
    }

    /// Cancela a transcrição quando ela chega à janela `at`.
    struct CancelAt {
        job: Arc<LocalTranscription>,
        at: u64,
    }
    impl ProgressListener for CancelAt {
        fn on_progress(&self, stage: Stage, done: u64, _: u64) {
            if stage == Stage::Transcribe && done == self.at {
                self.job.cancel();
            }
        }
    }

    #[test]
    #[ignore = "precisa dos modelos Whisper e Silero instalados"]
    fn transcreve_retoma_e_grava_a_ata() {
        let Some(model) = isper_models::resolve_whisper_model(None, false, &[]) else {
            return;
        };
        let Ok(vad) = isper_models::vad_path() else {
            return;
        };
        if !vad.exists() {
            return;
        }
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/reuniao-sintetica-16k.wav");
        let pcm = isper_core::audio::load_wav(&fixture)
            .unwrap()
            .into_whisper_input()
            .unwrap();
        let d = dir("transcreve");
        let id = "20260926-100000";
        gravar(&d, id, &pcm);
        let options = LocalOptions {
            model_path: model.display().to_string(),
            vad_path: vad.display().to_string(),
            lang: "pt".into(),
            diarize_models_dir: None,
        };

        // Primeira rodada: interrompida na terceira janela.
        let job = LocalTranscription::new();
        let err = job
            .run(
                d.display().to_string(),
                id.into(),
                options.clone(),
                Arc::new(CancelAt {
                    job: Arc::clone(&job),
                    at: 2,
                }),
            )
            .expect_err("interrompida");
        assert!(matches!(err, MobileError::Cancelled), "{err:?}");
        let parcial = local_progress(&d, id).expect("andamento guardado");
        assert!(parcial > 0.0 && parcial < 1.0, "{parcial}");
        assert!(minutes_origin(&d, id).is_none(), "sem ata ainda");

        // Segunda: continua de onde parou e grava a ata.
        let m = LocalTranscription::new()
            .run(
                d.display().to_string(),
                id.into(),
                options,
                Arc::new(Silent),
            )
            .expect("ata");
        assert_eq!(
            m.resumed_windows, 3,
            "as janelas 0, 1 e 2 já estavam feitas"
        );
        let md = std::fs::read_to_string(&m.minutes_path).unwrap();
        assert!(md.starts_with("# Reunião — 26/09/2026 10:00\n"), "{md}");
        assert!(md.contains("> Feita no celular, com o modelo"), "{md}");
        assert!(md.contains("**[00:0"), "com falas: {md}");
        assert_eq!(minutes_origin(&d, id), Some(MinutesOrigin::Device));
        assert!(
            !checkpoint_path(&d, id).exists(),
            "a retomada sai quando a ata fica pronta"
        );
        assert!(
            pending_local(d.display().to_string(), None)
                .unwrap()
                .is_empty()
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
