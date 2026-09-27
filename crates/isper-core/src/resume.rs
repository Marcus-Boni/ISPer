//! O que o passe final já transcreveu, guardado para continuar dali (Fase 9.4).
//!
//! No celular, o sistema pode parar a transcrição de uma reunião de horas a
//! qualquer momento: a tomada saiu, o Android precisou da memória, o app foi
//! fechado. Transcrever de novo desde o começo jogaria fora o que já foi
//! feito — e, numa reunião longa num aparelho lento, talvez nunca terminasse.
//!
//! O arquivo é JSON Lines:
//!
//! ```text
//! {"v":1,"rodada":"{…modelo, idioma, decodificação, janelas…}"}
//! {"i":0,"segments":[…],"carried":"…","last_word_end":12.3,…}
//! {"i":1,…}
//! ```
//!
//! O cabeçalho descreve a rodada inteira, como texto: é o texto que se compara,
//! porque um número com vírgula lido de volta de um JSON pode sair com o
//! último bit diferente, e aí nenhuma rodada bateria com a guardada. Se
//! qualquer coisa nela mudar — outro
//! modelo, outro idioma, outro áudio (as janelas do VAD mudam junto) —, o que
//! estava guardado não serve, e a transcrição recomeça do zero. Cada janela
//! concluída vira uma linha, com o estado que a próxima janela precisa (o
//! texto que vira contexto e onde terminou a última palavra), então continuar
//! dá exatamente o mesmo resultado de uma rodada sem interrupção. Uma última
//! linha pela metade (queda no meio da escrita) é descartada.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Result;
use crate::engine::TranscriptSegment;

/// Versão do formato. Outra versão no cabeçalho recomeça do zero.
const VERSION: u32 = 1;

/// A chave da descrição da rodada com a lista de janelas: é dela que sai o
/// total para [`progress`].
pub(crate) const WINDOWS_KEY: &str = "janelas";

/// Janelas entre um `sync_data` e outro. Sem ele, uma queda do aparelho pode
/// levar as últimas linhas; com ele a cada janela, o disco trabalharia à toa.
const SYNC_EVERY: usize = 8;

#[derive(Serialize, Deserialize)]
struct Header {
    v: u32,
    /// A descrição da rodada, serializada: compara-se o texto.
    rodada: String,
}

/// Uma janela concluída e o estado do passe final logo depois dela.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct WindowDone {
    /// Índice da janela.
    pub(crate) i: usize,
    /// Segmentos que a janela acrescentou, já no relógio do áudio.
    pub(crate) segments: Vec<TranscriptSegment>,
    /// O texto que serve de contexto à próxima janela.
    pub(crate) carried: Option<String>,
    /// Onde terminou a última palavra emitida até aqui.
    pub(crate) last_word_end: f32,
    /// Onde terminou a última janela que produziu texto.
    pub(crate) previous_window_end: f32,
    /// A janela não produziu texto.
    #[serde(default)]
    pub(crate) empty: bool,
    /// A inferência falhou nesta janela.
    #[serde(default)]
    pub(crate) failed: bool,
    /// Segmentos que o filtro de alucinações jogou fora.
    #[serde(default)]
    pub(crate) dropped: usize,
    /// A janela herdou contexto da anterior.
    #[serde(default)]
    pub(crate) with_context: bool,
}

/// O arquivo aberto: o que já estava lá e onde escrever o resto.
pub(crate) struct Checkpoint {
    file: File,
    /// Janelas que já estavam concluídas, em ordem.
    pub(crate) restored: Vec<WindowDone>,
    since_sync: usize,
}

impl Checkpoint {
    /// Abre (ou cria) o arquivo da rodada descrita por `rodada`. O que estava
    /// guardado de OUTRA rodada é apagado.
    pub(crate) fn open(path: &Path, rodada: &Value) -> Result<Self> {
        let rodada = serde_json::to_string(rodada).map_err(std::io::Error::other)?;
        let (restored, valid_len) = read_valid(path, &rodada);
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)?;
        // Corta o que não vale (outra rodada, ou uma linha pela metade) antes
        // de continuar escrevendo: senão a próxima linha sairia colada nela.
        file.set_len(valid_len)?;
        file.seek(SeekFrom::End(0))?;
        let mut me = Self {
            file,
            restored,
            since_sync: 0,
        };
        if valid_len == 0 {
            let header = Header { v: VERSION, rodada };
            me.append_line(&serde_json::to_string(&header).map_err(std::io::Error::other)?)?;
        }
        Ok(me)
    }

    /// Guarda uma janela concluída.
    pub(crate) fn push(&mut self, done: &WindowDone) -> Result<()> {
        let line = serde_json::to_string(done).map_err(std::io::Error::other)?;
        self.append_line(&line)?;
        self.since_sync += 1;
        if self.since_sync >= SYNC_EVERY {
            self.since_sync = 0;
            self.file.sync_data()?;
        }
        Ok(())
    }

    fn append_line(&mut self, line: &str) -> Result<()> {
        self.file.write_all(line.as_bytes())?;
        self.file.write_all(b"\n")?;
        self.file.flush()?;
        Ok(())
    }
}

/// Quantas janelas de quantas um arquivo já guarda, sem conferir a rodada: é
/// para mostrar o andamento ("42%"), não para continuar.
pub(crate) fn progress(path: &Path) -> Option<(usize, usize)> {
    let mut reader = BufReader::new(File::open(path).ok()?);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let header: Header = serde_json::from_str(line.trim_end()).ok()?;
    if header.v != VERSION {
        return None;
    }
    let rodada: Value = serde_json::from_str(&header.rodada).ok()?;
    let total = rodada.get(WINDOWS_KEY)?.as_array()?.len();
    let mut done = 0;
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(n) if n > 0 && line.ends_with('\n') => done += 1,
            _ => break,
        }
    }
    Some((done.min(total), total))
}

/// Lê o arquivo e devolve as janelas que valem para `rodada` e até que byte
/// ele vale (0 = nada aproveitável: arquivo ausente, de outra rodada ou de
/// outra versão).
fn read_valid(path: &Path, rodada: &str) -> (Vec<WindowDone>, u64) {
    let Ok(file) = File::open(path) else {
        return (Vec::new(), 0);
    };
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut valid: u64 = 0;

    // Cabeçalho.
    match reader.read_line(&mut line) {
        Ok(n) if n > 0 && line.ends_with('\n') => {}
        _ => return (Vec::new(), 0),
    }
    let same = serde_json::from_str::<Header>(line.trim_end())
        .is_ok_and(|h| h.v == VERSION && h.rodada == rodada);
    if !same {
        return (Vec::new(), 0);
    }
    valid += line.len() as u64;

    // Janelas, em ordem, até a primeira que não vale.
    let mut done = Vec::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            // Sem o "\n" final, a linha ficou pela metade: não vale.
            Ok(n) if n > 0 && line.ends_with('\n') => {}
            _ => break,
        }
        let Ok(w) = serde_json::from_str::<WindowDone>(line.trim_end()) else {
            break;
        };
        if w.i != done.len() {
            break;
        }
        valid += line.len() as u64;
        done.push(w);
    }
    (done, valid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Word;
    use serde_json::json;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("isper-resume-{}-{name}", std::process::id()));
        let _ = std::fs::remove_file(&d);
        d
    }

    fn window(i: usize, text: &str) -> WindowDone {
        WindowDone {
            i,
            segments: vec![TranscriptSegment {
                start_secs: i as f32 * 10.0,
                end_secs: i as f32 * 10.0 + 4.5,
                text: text.into(),
                no_speech_prob: 0.01,
                avg_logprob: -0.2,
                words: vec![Word {
                    text: text.into(),
                    start_secs: i as f32 * 10.0,
                    end_secs: i as f32 * 10.0 + 4.5,
                    prob: 0.9,
                }],
            }],
            carried: Some(text.into()),
            last_word_end: i as f32 * 10.0 + 4.5,
            previous_window_end: i as f32 * 10.0 + 5.0,
            empty: false,
            failed: false,
            dropped: 0,
            with_context: i > 0,
        }
    }

    #[test]
    fn continua_de_onde_parou() {
        let path = tmp("continua");
        let rodada = json!({"modelo": "small", "janelas": [[0.0, 10.0], [10.0, 20.0]]});
        let mut c = Checkpoint::open(&path, &rodada).unwrap();
        assert!(c.restored.is_empty());
        c.push(&window(0, "bom dia")).unwrap();
        c.push(&window(1, "a pauta")).unwrap();
        drop(c);

        let mut c = Checkpoint::open(&path, &rodada).unwrap();
        assert_eq!(c.restored, vec![window(0, "bom dia"), window(1, "a pauta")]);
        c.push(&window(2, "próximo passo")).unwrap();
        drop(c);
        let c = Checkpoint::open(&path, &rodada).unwrap();
        assert_eq!(c.restored.len(), 3);
        assert_eq!(c.restored[2].segments[0].words[0].text, "próximo passo");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn numeros_com_virgula_na_rodada_nao_atrapalham() {
        // 0,1 e 12,34 em f32 não têm representação exata: lidos de volta de
        // um JSON, podiam sair com o último bit diferente.
        let path = tmp("virgula");
        let janelas: Vec<(f32, f32, bool)> = (0..40)
            .map(|i| (i as f32 * 12.34 + 0.1, i as f32 * 12.34 + 9.87, i % 3 != 0))
            .collect();
        let rodada = json!({"confianca_minima": -1.0f32, WINDOWS_KEY: janelas});
        let mut c = Checkpoint::open(&path, &rodada).unwrap();
        c.push(&window(0, "um")).unwrap();
        drop(c);
        let c = Checkpoint::open(&path, &rodada).unwrap();
        assert_eq!(c.restored.len(), 1, "a mesma rodada tem de continuar");
        assert_eq!(progress(&path), Some((1, 40)));
        drop(c);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn linha_pela_metade_e_descartada_e_o_arquivo_segue_valido() {
        let path = tmp("metade");
        let rodada = json!({"modelo": "small"});
        let mut c = Checkpoint::open(&path, &rodada).unwrap();
        c.push(&window(0, "um")).unwrap();
        drop(c);
        // Queda no meio da escrita da janela 1.
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(br#"{"i":1,"segments":[{"start_se"#).unwrap();
        drop(f);

        let mut c = Checkpoint::open(&path, &rodada).unwrap();
        assert_eq!(c.restored, vec![window(0, "um")]);
        c.push(&window(1, "dois")).unwrap();
        drop(c);
        let c = Checkpoint::open(&path, &rodada).unwrap();
        assert_eq!(c.restored, vec![window(0, "um"), window(1, "dois")]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn outra_rodada_recomeca_do_zero() {
        let path = tmp("outra");
        let mut c = Checkpoint::open(&path, &json!({"modelo": "small"})).unwrap();
        c.push(&window(0, "um")).unwrap();
        drop(c);

        let c = Checkpoint::open(&path, &json!({"modelo": "base"})).unwrap();
        assert!(c.restored.is_empty(), "outro modelo não aproveita nada");
        drop(c);
        // E o arquivo passou a ser da rodada nova.
        let c = Checkpoint::open(&path, &json!({"modelo": "base"})).unwrap();
        assert!(c.restored.is_empty());
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 1, "só o cabeçalho novo: {text}");
        assert!(text.contains("base"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn janela_fora_de_ordem_para_a_leitura() {
        let path = tmp("ordem");
        let rodada = json!({"modelo": "small"});
        let mut c = Checkpoint::open(&path, &rodada).unwrap();
        c.push(&window(0, "um")).unwrap();
        c.push(&window(2, "três")).unwrap(); // pulou a 1: não pode valer
        drop(c);
        let c = Checkpoint::open(&path, &rodada).unwrap();
        assert_eq!(c.restored, vec![window(0, "um")]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn andamento_conta_as_janelas_guardadas() {
        let path = tmp("andamento");
        assert_eq!(progress(&path), None, "sem arquivo");
        let rodada =
            json!({"modelo": "small", WINDOWS_KEY: [[0, 10], [10, 20], [20, 30], [30, 40]]});
        let mut c = Checkpoint::open(&path, &rodada).unwrap();
        assert_eq!(progress(&path), Some((0, 4)));
        c.push(&window(0, "um")).unwrap();
        assert_eq!(progress(&path), Some((1, 4)));
        drop(c);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn arquivo_vazio_ou_estranho_vira_rodada_nova() {
        let path = tmp("estranho");
        std::fs::write(&path, "não é JSON\n").unwrap();
        let c = Checkpoint::open(&path, &json!({"modelo": "small"})).unwrap();
        assert!(c.restored.is_empty());
        drop(c);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with(r#"{"v":1"#), "{text}");
        let _ = std::fs::remove_file(&path);
    }
}
