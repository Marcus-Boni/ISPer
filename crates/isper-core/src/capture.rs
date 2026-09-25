//! Gravações feitas fora do PC (Fase 9.2): o áudio em Ogg/Opus
//! ([`crate::ogg_opus`]) e, ao lado, um manifesto JSON com o que o áudio
//! sozinho não diz — quando começou, os momentos marcados, os trechos em que o
//! microfone foi silenciado e se a gravação terminou bem.
//!
//! O par de arquivos é o artefato durável, como o `.md` das reuniões no PC
//! ([ADR 0009](../../docs/adr/0009-nada-some-sem-o-usuario-pedir.md)): a lista
//! de gravações sai da pasta, e um índice, quando existir, é reconstruível.
//!
//! O ciclo de vida:
//!
//! 1. [`CaptureRecorder::start`] grava o manifesto em estado
//!    [`CaptureState::Recording`] **antes** do primeiro byte de áudio;
//! 2. cada momento marcado e cada silêncio regravam o manifesto (escrita
//!    atômica: arquivo temporário e troca de nome);
//! 3. [`CaptureRecorder::finish`] fecha o Ogg e marca
//!    [`CaptureState::Finished`], com a duração.
//!
//! Se o app morrer no meio, o manifesto fica em `Recording`. Na próxima
//! abertura, [`recover`] mede o que o Ogg guardou (até a última página
//! íntegra) e marca [`CaptureState::Recovered`] — nada se perde além do último
//! segundo.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ogg_opus::{DEFAULT_BITRATE, OggOpusWriter};
use crate::{IsperError, Result};

/// Versão do formato do manifesto.
pub const MANIFEST_VERSION: u32 = 1;
/// Extensão do manifesto (o áudio tem o mesmo nome, com `.opus`).
pub const MANIFEST_EXT: &str = "json";

/// Em que pé está uma gravação.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureState {
    /// Gravando agora (ou o app caiu no meio — ver [`recover`]).
    Recording,
    /// Terminou normalmente.
    Finished,
    /// O app caiu no meio; o áudio vale até a última página íntegra.
    Recovered,
    /// Veio de fora (compartilhado de outro app), não gravado aqui.
    Imported,
}

/// Por que um trecho da gravação não tem a fala da sala.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapReason {
    /// O sistema deu o microfone a outro app (uma ligação, por exemplo) e a
    /// gravação recebeu silêncio.
    Silenced,
    /// Quem gravava pausou; o áudio não inclui o tempo pausado.
    Paused,
    /// O microfone parou de responder e foi reaberto.
    Reopened,
}

/// Um trecho anotado da gravação, em segundos do áudio.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Gap {
    /// Onde começa.
    pub start_secs: f64,
    /// Onde termina (igual ao início numa pausa, que não ocupa áudio).
    pub end_secs: f64,
    /// O motivo.
    pub reason: GapReason,
}

/// O manifesto de uma gravação (`<id>.json`, ao lado de `<id>.opus`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureManifest {
    /// Versão deste formato ([`MANIFEST_VERSION`]).
    pub version: u32,
    /// Identificador e nome base dos arquivos (ex.: `20260924-153046`).
    pub id: String,
    /// Início, em RFC 3339 com fuso (quem grava sabe o fuso; o núcleo não).
    pub started_at: String,
    /// Estado da gravação.
    pub state: CaptureState,
    /// Nome do arquivo de áudio, na mesma pasta.
    pub audio_file: String,
    /// Duração do áudio, quando conhecida.
    pub duration_secs: Option<f64>,
    /// Momentos marcados (★), em segundos do áudio.
    pub moments: Vec<f64>,
    /// Trechos silenciados, pausas e reaberturas.
    pub gaps: Vec<Gap>,
    /// Taxa em que o microfone gravou, em Hz.
    pub sample_rate: Option<u32>,
    /// Nome original, quando a gravação veio de outro app.
    pub source_name: Option<String>,
}

impl CaptureManifest {
    /// Caminho do manifesto de `id` em `dir`.
    pub fn path_in(dir: &Path, id: &str) -> PathBuf {
        dir.join(format!("{id}.{MANIFEST_EXT}"))
    }

    /// Caminho do áudio.
    pub fn audio_path(&self, dir: &Path) -> PathBuf {
        dir.join(&self.audio_file)
    }

    /// Grava o manifesto sem nunca deixar um arquivo pela metade: escreve ao
    /// lado e troca o nome.
    pub fn save(&self, dir: &Path) -> Result<()> {
        let path = Self::path_in(dir, &self.id);
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_vec_pretty(self)
            .map_err(|e| IsperError::Io(std::io::Error::other(e)))?;
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&json)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Lê um manifesto.
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        serde_json::from_slice(&bytes)
            .map_err(|e| IsperError::Decode(format!("manifesto {} ilegível: {e}", path.display())))
    }
}

/// Um nome de arquivo seguro para `id`: só letras, dígitos, `-` e `_`.
fn check_id(id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(IsperError::Audio(format!(
            "identificador de gravação inválido: {id:?}"
        )));
    }
    Ok(())
}

/// Grava uma reunião: o Ogg/Opus e o manifesto, sempre coerentes.
pub struct CaptureRecorder {
    dir: PathBuf,
    writer: Option<OggOpusWriter>,
    manifest: CaptureManifest,
    silenced_since: Option<f64>,
}

impl CaptureRecorder {
    /// Começa uma gravação em `dir` com o nome `id`. `started_at` é o início em
    /// RFC 3339; `rate`, a taxa do PCM que vai chegar.
    pub fn start(dir: &Path, id: &str, started_at: &str, rate: u32) -> Result<Self> {
        check_id(id)?;
        std::fs::create_dir_all(dir)?;
        let manifest = CaptureManifest {
            version: MANIFEST_VERSION,
            id: id.to_string(),
            started_at: started_at.to_string(),
            state: CaptureState::Recording,
            audio_file: format!("{id}.opus"),
            duration_secs: None,
            moments: Vec::new(),
            gaps: Vec::new(),
            sample_rate: Some(rate),
            source_name: None,
        };
        if CaptureManifest::path_in(dir, id).exists() {
            return Err(IsperError::Audio(format!("já existe uma gravação {id}")));
        }
        // O manifesto vem antes do áudio: se algo cair daqui em diante, a
        // recuperação sabe que havia uma gravação em andamento.
        manifest.save(dir)?;
        let writer = OggOpusWriter::create(&manifest.audio_path(dir), rate, DEFAULT_BITRATE)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            writer: Some(writer),
            manifest,
            silenced_since: None,
        })
    }

    /// O manifesto como está agora.
    pub fn manifest(&self) -> &CaptureManifest {
        &self.manifest
    }

    /// Segundos de áudio gravados.
    pub fn elapsed_secs(&self) -> f64 {
        self.writer
            .as_ref()
            .map_or(0.0, OggOpusWriter::elapsed_secs)
    }

    /// PCM mono 16 bits, little-endian (como o `AudioRecord` entrega).
    pub fn write_le_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        match self.writer.as_mut() {
            Some(w) => w.write_le_bytes(bytes),
            None => Err(IsperError::Audio("a gravação já terminou".into())),
        }
    }

    /// Marca um momento (★) no ponto atual. Devolve onde, em segundos.
    pub fn mark_moment(&mut self) -> Result<f64> {
        let at = self.elapsed_secs();
        self.manifest.moments.push(at);
        self.manifest.save(&self.dir)?;
        Ok(at)
    }

    /// O sistema silenciou (ou devolveu) o microfone. Abre ou fecha um trecho
    /// [`GapReason::Silenced`].
    pub fn set_silenced(&mut self, silenced: bool) -> Result<()> {
        let now = self.elapsed_secs();
        match (silenced, self.silenced_since) {
            (true, None) => self.silenced_since = Some(now),
            (false, Some(start)) => {
                self.silenced_since = None;
                self.manifest.gaps.push(Gap {
                    start_secs: start,
                    end_secs: now,
                    reason: GapReason::Silenced,
                });
                self.manifest.save(&self.dir)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Anota uma pausa ou uma reabertura do microfone no ponto atual.
    pub fn mark_gap(&mut self, reason: GapReason) -> Result<()> {
        let at = self.elapsed_secs();
        self.manifest.gaps.push(Gap {
            start_secs: at,
            end_secs: at,
            reason,
        });
        self.manifest.save(&self.dir)
    }

    /// Fecha o áudio e o manifesto. Devolve o manifesto final.
    pub fn finish(mut self) -> Result<CaptureManifest> {
        let _ = self.set_silenced(false);
        let writer = self
            .writer
            .take()
            .ok_or_else(|| IsperError::Audio("a gravação já terminou".into()))?;
        let secs = writer.finish()?;
        self.manifest.duration_secs = Some(secs);
        self.manifest.state = CaptureState::Finished;
        self.manifest.save(&self.dir)?;
        Ok(self.manifest)
    }
}

/// Todas as gravações de `dir`, da mais nova para a mais antiga (pelo id, que
/// começa pela data). Manifestos ilegíveis ficam de fora, com aviso no log.
pub fn list(dir: &Path) -> Result<Vec<CaptureManifest>> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e.into()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some(MANIFEST_EXT) {
            continue;
        }
        // Só `<id>.json` é manifesto: um id nunca tem ponto, e os arquivos de
        // outros módulos ao lado (`<id>.sync.json`, da sincronia) ficam de fora.
        let is_manifest = path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|stem| !stem.contains('.'));
        if !is_manifest {
            continue;
        }
        match CaptureManifest::load(&path) {
            Ok(m) => out.push(m),
            Err(e) => tracing::warn!("{e}"),
        }
    }
    out.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(out)
}

/// Fecha as gravações que ficaram em [`CaptureState::Recording`] porque o app
/// caiu — todas menos `active`, a que está gravando agora. Mede o áudio que
/// sobreviveu e marca [`CaptureState::Recovered`]. Devolve as recuperadas.
pub fn recover(dir: &Path, active: Option<&str>) -> Result<Vec<CaptureManifest>> {
    let mut recovered = Vec::new();
    for mut m in list(dir)? {
        if m.state != CaptureState::Recording || Some(m.id.as_str()) == active {
            continue;
        }
        let audio = m.audio_path(dir);
        m.duration_secs = match crate::ogg_opus::duration_secs(&audio) {
            Ok(secs) => Some(secs),
            Err(e) => {
                tracing::warn!("gravação {} sem áudio legível: {e}", m.id);
                Some(0.0)
            }
        };
        // Um silêncio que estava aberto termina onde o áudio termina.
        m.state = CaptureState::Recovered;
        m.save(dir)?;
        tracing::info!(id = %m.id, secs = ?m.duration_secs, "gravação recuperada");
        recovered.push(m);
    }
    Ok(recovered)
}

/// Traz um arquivo de outro app (o Plaud, o WhatsApp, o gravador do aparelho)
/// para `dir`, com um manifesto [`CaptureState::Imported`]. A duração vem do
/// próprio Ogg/Opus; para os outros formatos fica em aberto (medir exige
/// decodificar o arquivo inteiro).
pub fn import(
    dir: &Path,
    source: &Path,
    source_name: &str,
    id: &str,
    started_at: &str,
) -> Result<CaptureManifest> {
    check_id(id)?;
    // A extensão vem do nome original (é ele que diz o formato; o caminho de
    // origem costuma ser uma cópia temporária sem nome útil).
    let ext = Path::new(source_name)
        .extension()
        .or_else(|| source.extension())
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !crate::decode::AUDIO_EXTENSIONS.contains(&ext.as_str()) {
        return Err(IsperError::Decode(format!(
            "{source_name}: formato que o ISPer não lê (ele lê MP3, M4A, MP4, AAC, WAV, FLAC, OGG e Opus)"
        )));
    }
    std::fs::create_dir_all(dir)?;
    let audio_file = format!("{id}.{ext}");
    let dest = dir.join(&audio_file);
    if dest.exists() || CaptureManifest::path_in(dir, id).exists() {
        return Err(IsperError::Audio(format!("já existe uma gravação {id}")));
    }
    std::fs::copy(source, &dest)?;
    let duration_secs = if ext == "opus" {
        crate::ogg_opus::duration_secs(&dest).ok()
    } else {
        None
    };
    let manifest = CaptureManifest {
        version: MANIFEST_VERSION,
        id: id.to_string(),
        started_at: started_at.to_string(),
        state: CaptureState::Imported,
        audio_file,
        duration_secs,
        moments: Vec::new(),
        gaps: Vec::new(),
        sample_rate: None,
        source_name: Some(source_name.to_string()),
    };
    manifest.save(dir)?;
    Ok(manifest)
}

/// Apaga uma gravação (áudio e manifesto). Quem chama já confirmou — no app,
/// com "Desfazer" antes de chamar isto.
pub fn delete(dir: &Path, id: &str) -> Result<()> {
    check_id(id)?;
    let path = CaptureManifest::path_in(dir, id);
    let manifest = CaptureManifest::load(&path)?;
    let audio = manifest.audio_path(dir);
    if audio.exists() {
        std::fs::remove_file(audio)?;
    }
    std::fs::remove_file(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("isper-capture-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn um_segundo_de_tom(rate: u32) -> Vec<u8> {
        (0..rate)
            .flat_map(|i| {
                let s = ((i as f32 * 300.0 * std::f32::consts::TAU / rate as f32).sin() * 6_000.0)
                    as i16;
                s.to_le_bytes()
            })
            .collect()
    }

    #[test]
    fn gravacao_completa_com_momentos_e_silencio() {
        let d = dir("completa");
        let mut r =
            CaptureRecorder::start(&d, "20260924-150000", "2026-09-24T15:00:00-03:00", 16_000)
                .unwrap();
        // Manifesto existe desde o começo, em "gravando".
        let m = CaptureManifest::load(&CaptureManifest::path_in(&d, "20260924-150000")).unwrap();
        assert_eq!(m.state, CaptureState::Recording);

        r.write_le_bytes(&um_segundo_de_tom(16_000)).unwrap();
        let at = r.mark_moment().unwrap();
        assert!((at - 1.0).abs() < 1e-9);
        r.set_silenced(true).unwrap();
        r.write_le_bytes(&vec![0u8; 32_000]).unwrap(); // 1 s de silêncio imposto
        r.set_silenced(false).unwrap();
        r.mark_gap(GapReason::Paused).unwrap();
        r.write_le_bytes(&um_segundo_de_tom(16_000)).unwrap();
        let m = r.finish().unwrap();

        assert_eq!(m.state, CaptureState::Finished);
        assert!((m.duration_secs.unwrap() - 3.0).abs() < 1e-9);
        assert_eq!(m.moments, vec![1.0]);
        assert_eq!(m.gaps.len(), 2);
        assert_eq!(m.gaps[0].reason, GapReason::Silenced);
        assert!(
            (m.gaps[0].start_secs - 1.0).abs() < 1e-9 && (m.gaps[0].end_secs - 2.0).abs() < 1e-9
        );
        assert_eq!(m.gaps[1].reason, GapReason::Paused);
        // Os arquivos da sincronia (9.3) ao lado não são manifestos.
        std::fs::write(d.join("20260924-150000.sync.json"), r#"{"stage":"queued"}"#).unwrap();
        std::fs::write(
            d.join("20260924-150000.ata.md"),
            "# Ata
",
        )
        .unwrap();
        assert_eq!(list(&d).unwrap(), vec![m.clone()]);
        assert!(recover(&d, None).unwrap().is_empty(), "nada a recuperar");
        let decoded = crate::decode::decode_to_16k(&m.audio_path(&d), None, None).unwrap();
        assert!((decoded.duration_secs() - 3.0).abs() < 0.02);
    }

    #[test]
    fn queda_no_meio_e_recuperada_com_o_que_ficou_no_disco() {
        let d = dir("queda");
        let mut r =
            CaptureRecorder::start(&d, "20260924-160000", "2026-09-24T16:00:00-03:00", 48_000)
                .unwrap();
        for _ in 0..6 {
            r.write_le_bytes(&um_segundo_de_tom(48_000)).unwrap();
        }
        r.mark_moment().unwrap();
        // O app morre: nada de finish. Uma gravação nova, a ativa, não é tocada.
        drop(r);
        let ativa =
            CaptureRecorder::start(&d, "20260924-170000", "2026-09-24T17:00:00-03:00", 48_000)
                .unwrap();

        let rec = recover(&d, Some("20260924-170000")).unwrap();
        assert_eq!(rec.len(), 1);
        let m = &rec[0];
        assert_eq!(m.id, "20260924-160000");
        assert_eq!(m.state, CaptureState::Recovered);
        // 6 s gravados; as páginas fecham a cada segundo, então sobrevive
        // pelo menos 5 s (a página em andamento pode se perder).
        let secs = m.duration_secs.unwrap();
        assert!((5.0..=6.0).contains(&secs), "{secs} s recuperados");
        assert_eq!(m.moments.len(), 1, "os momentos já estavam no manifesto");
        // A lista mostra a nova primeiro, e ela continua gravando.
        let todas = list(&d).unwrap();
        assert_eq!(todas[0].id, "20260924-170000");
        assert_eq!(todas[0].state, CaptureState::Recording);
        drop(ativa);
    }

    #[test]
    fn importa_apaga_e_recusa_nomes_perigosos() {
        let d = dir("importa");
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/formatos/fala-2s.opus");
        let m = import(
            &d,
            &fixture,
            "PTT-20260924-WA0001.opus",
            "20260924-180000",
            "2026-09-24T18:00:00-03:00",
        )
        .unwrap();
        assert_eq!(m.state, CaptureState::Imported);
        assert_eq!(m.source_name.as_deref(), Some("PTT-20260924-WA0001.opus"));
        assert!((m.duration_secs.unwrap() - 2.0).abs() < 0.1);
        assert!(m.audio_path(&d).exists());

        assert!(import(&d, &fixture, "notas.txt", "20260924-180001", "x").is_err());
        assert!(CaptureRecorder::start(&d, "../fora", "x", 16_000).is_err());
        assert!(
            CaptureRecorder::start(&d, "20260924-180000", "x", 16_000).is_err(),
            "já existe"
        );

        delete(&d, "20260924-180000").unwrap();
        assert!(list(&d).unwrap().is_empty());
        assert!(!m.audio_path(&d).exists());
    }
}
