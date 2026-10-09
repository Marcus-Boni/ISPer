//! `isper-cli tarefas`: a extração de tarefas da fala (Fase 10.1), à mão e
//! medida num corpus.
//!
//! - `extrair "<fala>"`: roda a extração uma vez e mostra o que saiu, com as
//!   datas resolvidas;
//! - `corpus-ditados`: tira do banco os ditados reais para rotular;
//! - `avaliar <corpus.jsonl>`: mede precisão, recall, datas e latência de um
//!   provider/modelo no corpus rotulado.
//!
//! O corpus tem fala de verdade, então mora **fora do repositório**
//! (`Documentos\ISPer\Avaliacao-Tarefas`), como o do Jev. Uma linha por fala:
//!
//! ```json
//! {"id": "d-123", "hoje": "2026-10-07", "fala": "amanhã às 3 ligar pro João e pagar o boleto até sexta",
//!  "tarefas": [{"titulo": "Ligar pro João", "dia": "2026-10-08", "hora": "15:00", "prazo": null},
//!              {"titulo": "Pagar o boleto", "dia": null, "hora": null, "prazo": "2026-10-09"}]}
//! ```
//!
//! `tarefas: null` é fala ainda sem rótulo (fica de fora da conta);
//! `tarefas: []` é fala rotulada sem nenhuma tarefa.

use std::io::{BufRead, Write};
use std::path::Path;
use std::time::Instant;

use anyhow::Context;
use chrono::NaiveDate;
use isper_assist::NewTask;
use isper_assist::capture::draft;
use isper_core::store::MeetingStore;
use isper_llm::{ExtractedTask, LlmProvider, extract_tasks};
use serde::{Deserialize, Serialize};

/// O provider do `llm.toml`, ou o pedido na linha de comando.
pub fn provider(name: Option<&str>, model: Option<&str>) -> anyhow::Result<Box<dyn LlmProvider>> {
    let mut settings = isper_llm::load_settings();
    if let Some(p) = name {
        settings.provider = p.to_lowercase();
        settings.model = None;
    }
    if let Some(m) = model {
        settings.model = Some(m.to_string());
    }
    Ok(isper_llm::provider_from_settings(&settings)?)
}

fn today_or(hoje: Option<&str>) -> anyhow::Result<NaiveDate> {
    match hoje {
        Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .with_context(|| format!("data inválida em --hoje: {s} (use AAAA-MM-DD)")),
        None => Ok(chrono::Local::now().date_naive()),
    }
}

fn drafts(extracted: &[ExtractedTask], today: NaiveDate) -> Vec<NewTask> {
    extracted
        .iter()
        .map(|t| {
            draft(
                &t.title,
                t.when.as_deref(),
                t.due.as_deref(),
                t.notes.as_deref(),
                today,
            )
        })
        .collect()
}

/// `isper-cli tarefas extrair`.
pub fn extrair(
    fala: &str,
    name: Option<&str>,
    model: Option<&str>,
    hoje: Option<&str>,
) -> anyhow::Result<()> {
    let provider = provider(name, model)?;
    let today = today_or(hoje)?;
    let started = Instant::now();
    let extracted = extract_tasks(provider.as_ref(), fala)?;
    let ms = started.elapsed().as_millis();
    println!(
        "{} / {} · {} tarefa(s) em {ms} ms (hoje = {today})",
        provider.name(),
        provider.model(),
        extracted.len()
    );
    for (raw, t) in extracted.iter().zip(drafts(&extracted, today)) {
        let quando = match (&t.planned_on, &t.planned_time) {
            (Some(d), Some(h)) => format!("{d} {h}"),
            (Some(d), None) => d.to_string(),
            _ => "algum dia".into(),
        };
        let prazo = t
            .due_on
            .map(|d| format!(" · prazo {d}"))
            .unwrap_or_default();
        println!("- {} · {quando}{prazo}", t.title);
        if raw.when.is_some() || raw.due.is_some() {
            println!(
                "    (trechos: quando = {:?}, prazo = {:?})",
                raw.when.as_deref().unwrap_or("-"),
                raw.due.as_deref().unwrap_or("-")
            );
        }
        if !t.notes.is_empty() {
            println!("    notas: {}", t.notes);
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ corpus

/// Uma fala do corpus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    /// O dia em que a fala foi dita: "amanhã" é relativo a ele.
    pub hoje: NaiveDate,
    pub fala: String,
    /// O rótulo à mão; `None` = ainda não rotulada.
    pub tarefas: Option<Vec<Expected>>,
}

/// Uma tarefa esperada.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expected {
    pub titulo: String,
    #[serde(default)]
    pub dia: Option<NaiveDate>,
    #[serde(default)]
    pub hora: Option<String>,
    #[serde(default)]
    pub prazo: Option<NaiveDate>,
}

/// `isper-cli tarefas corpus-ditados`: os ditados do banco desde um dia,
/// sem rótulo, para rotular à mão. Não sobrescreve um corpus que exista.
pub fn corpus_ditados(db: &Path, desde: &str, saida: &Path) -> anyhow::Result<()> {
    let desde = NaiveDate::parse_from_str(desde, "%Y-%m-%d")
        .with_context(|| format!("data inválida em --desde: {desde} (use AAAA-MM-DD)"))?;
    anyhow::ensure!(
        !saida.exists(),
        "{} já existe: o rótulo de quem já trabalhou nele se perderia. Escolha outro --saida.",
        saida.display()
    );
    let store = MeetingStore::open(db)?;
    let rows = store.list_dictations(None, 100_000)?;
    if let Some(dir) = saida.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut out = std::io::BufWriter::new(std::fs::File::create(saida)?);
    let mut n = 0;
    for r in rows.iter().rev() {
        // `at` é "dd/mm/aaaa hh:mm:ss".
        let Some(dia) =
            r.at.get(..10)
                .and_then(|s| NaiveDate::parse_from_str(s, "%d/%m/%Y").ok())
        else {
            continue;
        };
        if dia < desde || r.text.trim().is_empty() {
            continue;
        }
        let item = Item {
            id: format!("d-{}", r.id),
            hoje: dia,
            fala: r.text.clone(),
            tarefas: None,
        };
        writeln!(out, "{}", serde_json::to_string(&item)?)?;
        n += 1;
    }
    out.flush()?;
    println!(
        "{n} ditado(s) desde {desde} em {}. Rotule cada linha preenchendo \"tarefas\".",
        saida.display()
    );
    Ok(())
}

pub fn read_corpus(path: &Path) -> anyhow::Result<Vec<Item>> {
    let file = std::fs::File::open(path).with_context(|| format!("abrir {}", path.display()))?;
    let mut items = Vec::new();
    for (i, line) in std::io::BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        items.push(
            serde_json::from_str(&line)
                .with_context(|| format!("linha {} de {}", i + 1, path.display()))?,
        );
    }
    Ok(items)
}

// --------------------------------------------------------------- avaliação

/// Palavras que não decidem se dois títulos são a mesma tarefa.
const STOP: &[&str] = &[
    "o", "a", "os", "as", "de", "do", "da", "dos", "das", "pro", "pra", "para", "com", "e", "em",
    "no", "na", "um", "uma", "ao", "sobre",
];

fn tokens(title: &str) -> std::collections::BTreeSet<String> {
    let folded: String = title
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'Á' | 'À' | 'Â' | 'Ã' => 'a',
            'é' | 'ê' | 'É' | 'Ê' => 'e',
            'í' | 'Í' => 'i',
            'ó' | 'ô' | 'õ' | 'Ó' | 'Ô' | 'Õ' => 'o',
            'ú' | 'ü' | 'Ú' | 'Ü' => 'u',
            'ç' | 'Ç' => 'c',
            c => c,
        })
        .flat_map(char::to_lowercase)
        .collect();
    folded
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !STOP.contains(w))
        .map(str::to_string)
        .collect()
}

/// Jaccard das palavras que importam: 1 é o mesmo título, 0 nada em comum.
pub fn similarity(a: &str, b: &str) -> f64 {
    let (a, b) = (tokens(a), tokens(b));
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let inter = a.intersection(&b).count() as f64;
    let union = a.union(&b).count() as f64;
    inter / union
}

/// A partir de quanto dois títulos são a mesma tarefa.
pub const MATCH_AT: f64 = 0.5;

/// Pares (esperada, extraída) casados pelo título, o mais parecido primeiro.
pub fn match_tasks(expected: &[Expected], got: &[NewTask]) -> Vec<(usize, usize)> {
    let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
    for (i, e) in expected.iter().enumerate() {
        for (j, g) in got.iter().enumerate() {
            let s = similarity(&e.titulo, &g.title);
            if s >= MATCH_AT {
                pairs.push((s, i, j));
            }
        }
    }
    pairs.sort_by(|x, y| y.0.total_cmp(&x.0));
    let (mut used_e, mut used_g) = (vec![false; expected.len()], vec![false; got.len()]);
    let mut out = Vec::new();
    for (_, i, j) in pairs {
        if !used_e[i] && !used_g[j] {
            used_e[i] = true;
            used_g[j] = true;
            out.push((i, j));
        }
    }
    out.sort();
    out
}

/// O placar de uma rodada.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct Score {
    pub falas: usize,
    pub sem_rotulo: usize,
    pub erros: usize,
    pub esperadas: usize,
    pub extraidas: usize,
    pub casadas: usize,
    pub dia_certo: usize,
    pub hora_certa: usize,
    pub prazo_certo: usize,
    pub latencias_ms: Vec<u128>,
}

impl Score {
    pub fn precision(&self) -> f64 {
        ratio(self.casadas, self.extraidas)
    }
    pub fn recall(&self) -> f64 {
        ratio(self.casadas, self.esperadas)
    }
    pub fn f1(&self) -> f64 {
        let (p, r) = (self.precision(), self.recall());
        if p + r == 0.0 {
            0.0
        } else {
            2.0 * p * r / (p + r)
        }
    }
    /// Percentil por posto mais próximo, em ms.
    pub fn latency(&self, p: f64) -> u128 {
        let mut v = self.latencias_ms.clone();
        if v.is_empty() {
            return 0;
        }
        v.sort_unstable();
        let rank = ((p / 100.0) * v.len() as f64).ceil() as usize;
        v[rank.clamp(1, v.len()) - 1]
    }

    /// Soma uma fala ao placar.
    pub fn add(&mut self, expected: &[Expected], got: &[NewTask]) {
        self.esperadas += expected.len();
        self.extraidas += got.len();
        for (i, j) in match_tasks(expected, got) {
            self.casadas += 1;
            let (e, g) = (&expected[i], &got[j]);
            self.dia_certo += usize::from(e.dia == g.planned_on);
            self.hora_certa += usize::from(e.hora == g.planned_time);
            self.prazo_certo += usize::from(e.prazo == g.due_on);
        }
    }
}

fn ratio(a: usize, b: usize) -> f64 {
    if b == 0 { 1.0 } else { a as f64 / b as f64 }
}

/// `isper-cli tarefas avaliar`.
pub fn avaliar(
    corpus: &Path,
    name: Option<&str>,
    model: Option<&str>,
    saida: Option<&Path>,
    limite: Option<usize>,
) -> anyhow::Result<()> {
    let provider = provider(name, model)?;
    let items = read_corpus(corpus)?;
    let mut score = Score::default();
    let mut report = Vec::new();
    for item in items.iter().take(limite.unwrap_or(usize::MAX)) {
        score.falas += 1;
        let Some(expected) = &item.tarefas else {
            score.sem_rotulo += 1;
            continue;
        };
        let started = Instant::now();
        let result = extract_tasks(provider.as_ref(), &item.fala);
        let ms = started.elapsed().as_millis();
        match result {
            Ok(extracted) => {
                score.latencias_ms.push(ms);
                let got = drafts(&extracted, item.hoje);
                score.add(expected, &got);
                report.push(serde_json::json!({
                    "id": item.id, "fala": item.fala, "ms": ms,
                    "esperadas": expected,
                    "extraidas": got.iter().map(|g| serde_json::json!({
                        "titulo": g.title, "dia": g.planned_on, "hora": g.planned_time, "prazo": g.due_on,
                    })).collect::<Vec<_>>(),
                    "casadas": match_tasks(expected, &got),
                }));
            }
            Err(e) => {
                score.erros += 1;
                report.push(serde_json::json!({"id": item.id, "erro": e.to_string()}));
            }
        }
        print!(".");
        let _ = std::io::stdout().flush();
    }
    println!();
    let pct = |a: usize, b: usize| 100.0 * ratio(a, b);
    println!("{} / {}", provider.name(), provider.model());
    println!(
        "falas: {} rotuladas de {} ({} sem rótulo) · erros de API: {}",
        score.falas - score.sem_rotulo,
        score.falas,
        score.sem_rotulo,
        score.erros
    );
    println!(
        "tarefas: precisão {:.2} · recall {:.2} · F1 {:.2} (esperadas {}, extraídas {}, casadas {})",
        score.precision(),
        score.recall(),
        score.f1(),
        score.esperadas,
        score.extraidas,
        score.casadas
    );
    println!(
        "nas casadas: dia certo {:.0}% · hora certa {:.0}% · prazo certo {:.0}%",
        pct(score.dia_certo, score.casadas),
        pct(score.hora_certa, score.casadas),
        pct(score.prazo_certo, score.casadas)
    );
    println!(
        "latência: p50 {} ms · p95 {} ms",
        score.latency(50.0),
        score.latency(95.0)
    );
    if let Some(path) = saida {
        let doc = serde_json::json!({
            "provider": provider.name(), "modelo": provider.model(),
            "placar": score, "precisao": score.precision(), "recall": score.recall(), "f1": score.f1(),
            "p50_ms": score.latency(50.0), "p95_ms": score.latency(95.0),
            "falas": report,
        });
        std::fs::write(path, serde_json::to_string_pretty(&doc)?)?;
        println!("relatório: {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(titulo: &str, dia: Option<&str>) -> Expected {
        Expected {
            titulo: titulo.into(),
            dia: dia.map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap()),
            hora: None,
            prazo: None,
        }
    }

    fn g(title: &str, dia: Option<&str>) -> NewTask {
        NewTask {
            planned_on: dia.map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap()),
            ..NewTask::titled(title)
        }
    }

    #[test]
    fn titulos_parecidos_casam_e_diferentes_nao() {
        assert!(similarity("Ligar pro João", "ligar para o joao") >= MATCH_AT);
        assert!(similarity("Revisar o contrato", "Revisar contrato do cliente") >= MATCH_AT);
        assert!(similarity("Pagar o boleto", "Ligar pro João") < MATCH_AT);
    }

    #[test]
    fn casamento_e_um_para_um() {
        let exp = [e("Revisar o PR", None), e("Revisar o PR do portal", None)];
        let got = [g("Revisar o PR do portal", None)];
        let m = match_tasks(&exp, &got);
        assert_eq!(m, vec![(1, 0)], "o mais parecido leva");
    }

    #[test]
    fn placar_de_precisao_recall_e_datas() {
        let mut s = Score::default();
        let exp = [
            e("Ligar pro João", Some("2026-10-08")),
            e("Pagar o boleto", None),
        ];
        let got = [
            g("Ligar pro João", Some("2026-10-08")),
            g("Pagar o boleto", Some("2026-10-07")),
            g("Inventada", None),
        ];
        s.add(&exp, &got);
        assert_eq!((s.esperadas, s.extraidas, s.casadas), (2, 3, 2));
        assert!((s.precision() - 2.0 / 3.0).abs() < 1e-9);
        assert!((s.recall() - 1.0).abs() < 1e-9);
        assert_eq!(s.dia_certo, 1, "o boleto ganhou um dia que não tinha");
        s.latencias_ms = vec![100, 300, 200, 1000];
        assert_eq!(s.latency(50.0), 200);
        assert_eq!(s.latency(95.0), 1000);
    }

    #[test]
    fn fala_sem_tarefa_rotulada_vazia_nao_derruba_a_conta() {
        let mut s = Score::default();
        s.add(&[], &[]);
        assert_eq!(s.precision(), 1.0);
        assert_eq!(s.recall(), 1.0);
    }

    #[test]
    fn linha_do_corpus_le_com_e_sem_rotulo() {
        let rot: Item = serde_json::from_str(
            r#"{"id":"d-1","hoje":"2026-10-07","fala":"x","tarefas":[{"titulo":"Ligar","dia":"2026-10-08"}]}"#,
        )
        .unwrap();
        assert_eq!(rot.tarefas.unwrap()[0].hora, None);
        let sem: Item =
            serde_json::from_str(r#"{"id":"d-2","hoje":"2026-10-07","fala":"x","tarefas":null}"#)
                .unwrap();
        assert!(sem.tarefas.is_none());
    }
}
