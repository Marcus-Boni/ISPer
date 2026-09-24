//! O gravador do celular (Fase 9.2), exposto ao app: o `AudioRecord` do
//! Android entrega PCM, e aqui ele vira Ogg/Opus com o manifesto ao lado
//! ([`isper_core::capture`]). O iOS vai usar o mesmo.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use isper_core::capture::{self, CaptureManifest, CaptureRecorder, CaptureState, GapReason};

use crate::MobileError;

/// Em que pé está uma gravação.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum RecordingState {
    /// Gravando agora.
    Recording,
    /// Terminou normalmente.
    Finished,
    /// O app caiu no meio; vale o áudio até a queda.
    Recovered,
    /// Veio de outro app.
    Imported,
}

impl From<CaptureState> for RecordingState {
    fn from(s: CaptureState) -> Self {
        match s {
            CaptureState::Recording => Self::Recording,
            CaptureState::Finished => Self::Finished,
            CaptureState::Recovered => Self::Recovered,
            CaptureState::Imported => Self::Imported,
        }
    }
}

/// Por que um trecho não tem a fala da sala.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum GapKind {
    /// Outro app (uma ligação) ficou com o microfone.
    Silenced,
    /// Pausa de quem gravava.
    Paused,
    /// O microfone parou e foi reaberto.
    Reopened,
}

impl From<GapReason> for GapKind {
    fn from(r: GapReason) -> Self {
        match r {
            GapReason::Silenced => Self::Silenced,
            GapReason::Paused => Self::Paused,
            GapReason::Reopened => Self::Reopened,
        }
    }
}

/// Um trecho anotado, em segundos do áudio.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RecordingGap {
    /// Início.
    pub start_secs: f64,
    /// Fim.
    pub end_secs: f64,
    /// Motivo.
    pub kind: GapKind,
}

/// Uma gravação, como a Biblioteca mostra.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RecordingInfo {
    /// Identificador (o nome base dos arquivos).
    pub id: String,
    /// Início, em RFC 3339.
    pub started_at: String,
    /// Estado.
    pub state: RecordingState,
    /// Caminho do áudio.
    pub audio_path: String,
    /// Duração, quando conhecida.
    pub duration_secs: Option<f64>,
    /// Tamanho do áudio, em bytes.
    pub size_bytes: u64,
    /// Momentos marcados (★), em segundos.
    pub moments: Vec<f64>,
    /// Silêncios impostos, pausas e reaberturas.
    pub gaps: Vec<RecordingGap>,
    /// Nome original, se veio de outro app.
    pub source_name: Option<String>,
}

fn info(dir: &Path, m: &CaptureManifest) -> RecordingInfo {
    let audio = m.audio_path(dir);
    RecordingInfo {
        id: m.id.clone(),
        started_at: m.started_at.clone(),
        state: m.state.into(),
        size_bytes: std::fs::metadata(&audio).map(|md| md.len()).unwrap_or(0),
        audio_path: audio.display().to_string(),
        duration_secs: m.duration_secs,
        moments: m.moments.clone(),
        gaps: m
            .gaps
            .iter()
            .map(|g| RecordingGap {
                start_secs: g.start_secs,
                end_secs: g.end_secs,
                kind: g.reason.into(),
            })
            .collect(),
        source_name: m.source_name.clone(),
    }
}

/// A lista da Biblioteca e o que foi recuperado ao montá-la.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RecordingList {
    /// Da mais nova para a mais antiga.
    pub recordings: Vec<RecordingInfo>,
    /// Gravações que o app tinha deixado em aberto (queda) e que foram
    /// fechadas agora.
    pub recovered: Vec<RecordingInfo>,
}

/// Uma gravação em andamento. Os métodos podem ser chamados de threads
/// diferentes (a de áudio escreve; a da interface marca momentos).
#[derive(uniffi::Object)]
pub struct Recorder {
    dir: PathBuf,
    inner: Mutex<Option<CaptureRecorder>>,
}

impl Recorder {
    fn lock(&self) -> MutexGuard<'_, Option<CaptureRecorder>> {
        // Um pânico noutra thread não pode custar a gravação inteira.
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn with<T>(
        &self,
        f: impl FnOnce(&mut CaptureRecorder) -> isper_core::Result<T>,
    ) -> Result<T, MobileError> {
        match self.lock().as_mut() {
            Some(r) => f(r).map_err(MobileError::from),
            None => Err(MobileError::Audio("a gravação já terminou".into())),
        }
    }
}

#[uniffi::export]
impl Recorder {
    /// Começa a gravar em `dir`. `id` é o nome base dos arquivos (a data e a
    /// hora, por exemplo `20260924-153046`), `started_at` o início em RFC
    /// 3339 e `sample_rate` a taxa do `AudioRecord` (48 000 ou 16 000).
    #[uniffi::constructor]
    pub fn start(
        dir: String,
        id: String,
        started_at: String,
        sample_rate: u32,
    ) -> Result<Arc<Self>, MobileError> {
        let dir = PathBuf::from(dir);
        let rec = CaptureRecorder::start(&dir, &id, &started_at, sample_rate)?;
        Ok(Arc::new(Self {
            dir,
            inner: Mutex::new(Some(rec)),
        }))
    }

    /// PCM mono 16 bits little-endian, como o `AudioRecord` entrega.
    pub fn write(&self, pcm: Vec<u8>) -> Result<(), MobileError> {
        self.with(|r| r.write_le_bytes(&pcm))
    }

    /// Segundos de áudio gravados.
    pub fn elapsed_secs(&self) -> f64 {
        self.lock()
            .as_ref()
            .map_or(0.0, CaptureRecorder::elapsed_secs)
    }

    /// Marca um momento (★) agora. Devolve onde, em segundos.
    pub fn mark_moment(&self) -> Result<f64, MobileError> {
        self.with(CaptureRecorder::mark_moment)
    }

    /// O sistema silenciou (ou devolveu) o microfone.
    pub fn set_silenced(&self, silenced: bool) -> Result<(), MobileError> {
        self.with(|r| r.set_silenced(silenced))
    }

    /// Quem grava pausou (o áudio não inclui o tempo pausado).
    pub fn mark_pause(&self) -> Result<(), MobileError> {
        self.with(|r| r.mark_gap(GapReason::Paused))
    }

    /// O microfone parou de responder e foi reaberto.
    pub fn mark_reopened(&self) -> Result<(), MobileError> {
        self.with(|r| r.mark_gap(GapReason::Reopened))
    }

    /// Fecha a gravação e devolve como ela ficou.
    pub fn finish(&self) -> Result<RecordingInfo, MobileError> {
        let rec = self
            .lock()
            .take()
            .ok_or_else(|| MobileError::Audio("a gravação já terminou".into()))?;
        let manifest = rec.finish()?;
        Ok(info(&self.dir, &manifest))
    }
}

/// As gravações de `dir`. Antes de listar, fecha as que o app deixou abertas
/// numa queda — todas menos `active_id`, a que está gravando agora.
#[uniffi::export]
pub fn list_recordings(
    dir: String,
    active_id: Option<String>,
) -> Result<RecordingList, MobileError> {
    let dir = PathBuf::from(dir);
    let recovered = capture::recover(&dir, active_id.as_deref())?;
    let recordings = capture::list(&dir)?;
    Ok(RecordingList {
        recordings: recordings.iter().map(|m| info(&dir, m)).collect(),
        recovered: recovered.iter().map(|m| info(&dir, m)).collect(),
    })
}

/// Traz para `dir` um áudio compartilhado por outro app (o Plaud, o
/// WhatsApp, o gravador do aparelho). `source_path` é a cópia que o app fez;
/// `source_name`, o nome original (é dele que sai o formato).
#[uniffi::export]
pub fn import_recording(
    dir: String,
    source_path: String,
    source_name: String,
    id: String,
    started_at: String,
) -> Result<RecordingInfo, MobileError> {
    let dir = PathBuf::from(dir);
    let m = capture::import(
        &dir,
        Path::new(&source_path),
        &source_name,
        &id,
        &started_at,
    )?;
    Ok(info(&dir, &m))
}

/// Apaga uma gravação (áudio e manifesto). O app oferece "Desfazer" antes.
#[uniffi::export]
pub fn delete_recording(dir: String, id: String) -> Result<(), MobileError> {
    capture::delete(Path::new(&dir), &id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> String {
        let d =
            std::env::temp_dir().join(format!("isper-mobile-rec-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.display().to_string()
    }

    #[test]
    fn grava_marca_e_lista() {
        let d = dir("lista");
        let r = Recorder::start(
            d.clone(),
            "20260924-190000".into(),
            "2026-09-24T19:00:00-03:00".into(),
            16_000,
        )
        .unwrap();
        r.write(vec![0u8; 32_000]).unwrap();
        let at = r.mark_moment().unwrap();
        assert!((at - 1.0).abs() < 1e-9);
        r.mark_pause().unwrap();
        r.write(vec![0u8; 16_000]).unwrap();
        assert!((r.elapsed_secs() - 1.5).abs() < 1e-9);
        let info = r.finish().unwrap();
        assert_eq!(info.state, RecordingState::Finished);
        assert!((info.duration_secs.unwrap() - 1.5).abs() < 1e-9);
        assert!(info.size_bytes > 0);
        assert!(r.write(vec![0u8; 2]).is_err(), "depois do fim não grava");

        let l = list_recordings(d.clone(), None).unwrap();
        assert_eq!(l.recordings, vec![info]);
        assert!(l.recovered.is_empty());
        delete_recording(d.clone(), "20260924-190000".into()).unwrap();
        assert!(list_recordings(d, None).unwrap().recordings.is_empty());
    }
}
