//! As ações de "Eu" numa reunião gravada (Fase 10.3, [ADR 0021]).
//!
//! Quando a reunião acaba, o que a pessoa que gravou ficou de fazer vai para
//! a caixa de entrada da tela Hoje, com o minuto em que foi dito. A LLM lê a
//! transcrição com o minuto de cada fala (`[12:34] Eu: …`) e devolve, num
//! JSON de esquema estrito, só as ações de "Eu": o que "Eu" se comprometeu a
//! fazer e o que pediram a "Eu" sem recusa. Ela copia o marcador do minuto e
//! o trecho do prazo como foram ditos; quem confere o minuto contra a
//! transcrição de verdade e resolve a data é o código.
//!
//! Reunião longa vai em pedaços: um modelo lê pior o meio de um texto
//! enorme, e uma ação dita na primeira hora não pode sumir.
//!
//! [ADR 0021]: ../../../docs/adr/0021-assistente-pessoal-no-isper.md

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::providers::LlmProvider;
use crate::tasks::{RETRY_WAITS, with_retries};
use crate::{LlmError, Result};

/// Quanto de transcrição vai em cada chamada.
const CHUNK_CHARS: usize = 40_000;
/// Falas do pedaço anterior que entram de novo no seguinte: um pedido feito
/// no fim de um pedaço e aceito no começo do outro não se perde.
const OVERLAP_LINES: usize = 6;
/// Mais que isso numa reunião só é sinal de que a LLM saiu do combinado.
pub const MAX_ACTIONS: usize = 30;
/// Título mais longo aceito (caracteres).
const MAX_TITLE_CHARS: usize = 200;
/// Trecho citado mais longo guardado (caracteres).
const MAX_QUOTE_CHARS: usize = 300;
/// Até onde o minuto que a LLM copiou pode cair de uma fala real.
const SNAP_SECS: f32 = 90.0;

/// Uma fala da transcrição, com o instante em que começou.
#[derive(Debug, Clone, PartialEq)]
pub struct TimedLine {
    /// Segundos desde o começo da reunião.
    pub at_secs: f32,
    /// "Eu", "Participante 2", um nome dado pelo usuário…
    pub speaker: String,
    /// O que foi dito.
    pub text: String,
}

/// Uma ação de "Eu", com o minuto conferido na transcrição.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MyAction {
    /// O que fazer, começando pelo verbo.
    pub title: String,
    /// O instante da fala em que a ação apareceu, já ajustado a uma fala
    /// real; `None` se a LLM não deu um minuto que exista.
    pub at_secs: Option<f32>,
    /// O trecho do prazo como foi dito ("até sexta"), sem calcular a data.
    pub due: Option<String>,
    /// A frase em que a ação aparece.
    pub quote: Option<String>,
}

const SYSTEM: &str = "Você lê a transcrição de uma reunião de trabalho e lista as tarefas que ficaram para \"Eu\", a pessoa que gravou. A transcrição é automática e pode ter erros; cada linha começa com o minuto da fala entre colchetes e quem falou.

Regras:
1. Só ações de \"Eu\": o que \"Eu\" disse que vai fazer (\"eu mando\", \"deixa comigo\", \"fico de ver\", \"vou levantar isso\") e o que outra pessoa pediu a \"Eu\" (pelo nome de \"Eu\", se ele aparecer, ou falando diretamente com quem gravou) sem \"Eu\" recusar.
2. Fora: tarefa de outra pessoa, tarefa do grupo sem dono, decisão, opinião, o que já foi feito durante a reunião, hipótese (\"talvez a gente pudesse\").
3. title: curto, começando por um verbo no infinitivo, com o objeto e os nomes (\"Mandar a planilha de estimativas para a Ana\"). Sem data no título.
4. at: o marcador de minuto da linha em que a ação aparece, copiado exatamente como está, sem os colchetes (\"12:34\" ou \"1:02:10\").
5. due: o trecho exato do prazo, como foi dito (\"até sexta\", \"amanhã cedo\", \"semana que vem\"), sem calcular a data. null se não houve prazo.
6. quote: a frase da transcrição em que a ação aparece, copiada, até 200 caracteres.
7. A mesma ação dita várias vezes vira uma só, no minuto em que ficou combinada.
8. Escreva no idioma da reunião. Sem nenhuma ação de \"Eu\", devolva a lista vazia.";

/// O esquema da resposta (JSON Schema estrito).
pub fn actions_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "actions": {
                "type": "array",
                "description": "As ações de \"Eu\", na ordem em que aparecem.",
                "items": {
                    "type": "object",
                    "properties": {
                        "title": {"type": "string", "description": "O que fazer, começando pelo verbo no infinitivo."},
                        "at": {"type": "string", "description": "O marcador de minuto da linha, sem colchetes."},
                        "due": {"type": ["string", "null"], "description": "Trecho exato do prazo, ou null."},
                        "quote": {"type": "string", "description": "A frase em que a ação aparece."},
                    },
                    "required": ["title", "at", "due", "quote"],
                    "additionalProperties": false,
                },
            },
        },
        "required": ["actions"],
        "additionalProperties": false,
    })
}

/// `754.2` → `"12:34"`; com hora, `"1:02:10"`.
pub fn clock(secs: f32) -> String {
    let s = secs.max(0.0) as u64;
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// `"12:34"`, `"[1:02:10]"`, `"12m34s"` → segundos.
fn parse_clock(text: &str) -> Option<f32> {
    let t = text.trim().trim_matches(|c| c == '[' || c == ']').trim();
    let t = t.replace(['h', 'm'], ":").replace('s', "");
    let parts: Vec<u32> = t
        .split(':')
        .filter(|p| !p.is_empty())
        .map(|p| p.trim().parse().ok())
        .collect::<Option<Vec<_>>>()?;
    let secs = match parts.as_slice() {
        [m, s] if *s < 60 => m * 60 + s,
        [h, m, s] if *m < 60 && *s < 60 => h * 3600 + m * 60 + s,
        _ => return None,
    };
    Some(secs as f32)
}

/// Os pedaços da transcrição, cada um com até [`CHUNK_CHARS`] e
/// [`OVERLAP_LINES`] falas repetidas do anterior.
fn chunks(lines: &[TimedLine]) -> Vec<String> {
    let rendered: Vec<String> = lines
        .iter()
        .map(|l| {
            format!(
                "[{}] {}: {}",
                clock(l.at_secs),
                l.speaker.trim(),
                l.text.split_whitespace().collect::<Vec<_>>().join(" ")
            )
        })
        .collect();
    let mut out = Vec::new();
    let mut start = 0;
    while start < rendered.len() {
        let mut size = 0;
        let mut end = start;
        while end < rendered.len() && (end == start || size + rendered[end].len() < CHUNK_CHARS) {
            size += rendered[end].len() + 1;
            end += 1;
        }
        out.push(rendered[start..end].join("\n"));
        if end >= rendered.len() {
            break;
        }
        start = end.saturating_sub(OVERLAP_LINES).max(start + 1);
    }
    out
}

/// As ações de "Eu" numa transcrição. `me` é o nome de quem gravou, quando
/// se sabe (é por ele que os outros pedem). Transcrição vazia nem chama a API.
pub fn extract_my_actions(
    provider: &dyn LlmProvider,
    lines: &[TimedLine],
    me: Option<&str>,
) -> Result<Vec<MyAction>> {
    let lines: Vec<TimedLine> = lines
        .iter()
        .filter(|l| !l.text.trim().is_empty())
        .cloned()
        .collect();
    if lines.is_empty() {
        return Ok(Vec::new());
    }
    let who = match me.map(str::trim).filter(|n| !n.is_empty()) {
        Some(name) => format!("\"Eu\" é {name}; os outros podem chamá-lo pelo nome."),
        None => "O nome de \"Eu\" não é conhecido.".to_string(),
    };
    let schema = actions_schema();
    let mut found = Vec::new();
    for chunk in chunks(&lines) {
        let user = format!("{who}\n\nTranscrição:\n<<<\n{chunk}\n>>>");
        let value = with_retries(&RETRY_WAITS, || {
            provider.complete_json(SYSTEM, &user, &schema)
        })?;
        found.extend(parse_actions(&value, &lines)?);
    }
    Ok(merge(found))
}

/// Lê a resposta: títulos aparados, o minuto ajustado à fala real mais
/// próxima (ou descartado), trechos vazios viram `None`.
pub fn parse_actions(value: &Value, lines: &[TimedLine]) -> Result<Vec<MyAction>> {
    let items = value["actions"]
        .as_array()
        .ok_or_else(|| LlmError::BadResponse("resposta sem a lista de ações".into()))?;
    let text = |v: &Value| {
        v.as_str()
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|s| !s.is_empty() && s != "null")
    };
    let mut out = Vec::new();
    for item in items {
        let Some(title) = text(&item["title"]) else {
            continue;
        };
        let title: String = title.chars().take(MAX_TITLE_CHARS).collect();
        let at_secs = item["at"]
            .as_str()
            .and_then(parse_clock)
            .and_then(|at| snap(at, lines));
        let quote = text(&item["quote"]).map(|q| q.chars().take(MAX_QUOTE_CHARS).collect());
        out.push(MyAction {
            title,
            at_secs,
            due: text(&item["due"]),
            quote,
        });
    }
    Ok(out)
}

/// O começo da fala mais próxima de `at`, se houver uma a até
/// [`SNAP_SECS`]. O marcador que a LLM copia é o da linha (arredondado ao
/// segundo); o ajuste devolve o instante exato e descarta minuto inventado.
fn snap(at: f32, lines: &[TimedLine]) -> Option<f32> {
    lines
        .iter()
        .map(|l| l.at_secs)
        .min_by(|a, b| (a - at).abs().total_cmp(&(b - at).abs()))
        .filter(|best| (best - at).abs() <= SNAP_SECS)
}

/// Palavras de um título para comparar (sem caixa, sem acento, sem as
/// palavras curtas).
fn words(title: &str) -> std::collections::BTreeSet<String> {
    title
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' => 'a',
            'é' | 'ê' => 'e',
            'í' => 'i',
            'ó' | 'ô' | 'õ' => 'o',
            'ú' | 'ü' => 'u',
            'ç' => 'c',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect::<String>()
        .split_whitespace()
        .filter(|w| w.len() > 2)
        .map(String::from)
        .collect()
}

/// Quanto dois títulos se parecem (Jaccard das palavras), de 0 a 1.
pub fn similarity(a: &str, b: &str) -> f32 {
    let (a, b) = (words(a), words(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(&b).count() as f32;
    inter / ((a.len() + b.len()) as f32 - inter)
}

/// Junta as repetidas (a mesma ação vista em dois pedaços, ou dita duas
/// vezes): fica a primeira, com o prazo de quem tiver. Ordena pelo minuto.
fn merge(actions: Vec<MyAction>) -> Vec<MyAction> {
    let mut out: Vec<MyAction> = Vec::new();
    for a in actions {
        if let Some(prev) = out
            .iter_mut()
            .find(|p| similarity(&p.title, &a.title) >= 0.6)
        {
            if prev.due.is_none() {
                prev.due = a.due;
            }
            if prev.at_secs.is_none() {
                prev.at_secs = a.at_secs;
            }
            continue;
        }
        out.push(a);
    }
    out.sort_by(|a, b| {
        a.at_secs
            .unwrap_or(f32::MAX)
            .total_cmp(&b.at_secs.unwrap_or(f32::MAX))
    });
    out.truncate(MAX_ACTIONS);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeProvider;

    fn line(at: f32, speaker: &str, text: &str) -> TimedLine {
        TimedLine {
            at_secs: at,
            speaker: speaker.into(),
            text: text.into(),
        }
    }

    fn reuniao() -> Vec<TimedLine> {
        vec![
            line(5.0, "Participante 1", "Bom dia, vamos começar pelo portal."),
            line(
                754.6,
                "Ana",
                "Marcus, você me manda a planilha de estimativas até sexta?",
            ),
            line(760.2, "Eu", "Mando sim, deixa comigo."),
            line(
                1210.0,
                "Participante 1",
                "Eu vou ver o deploy com o time de infra.",
            ),
        ]
    }

    #[test]
    fn a_transcricao_vai_com_o_minuto_e_o_minuto_volta_conferido() {
        let fake = FakeProvider::replying(
            r#"{"actions":[
                {"title":"Mandar a planilha de estimativas para a Ana","at":"12:34","due":"até sexta","quote":"Marcus, você me manda a planilha de estimativas até sexta?"}
            ]}"#,
        );
        let out = extract_my_actions(&fake, &reuniao(), Some("Marcus")).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].at_secs,
            Some(754.6),
            "o minuto vira o instante exato da fala"
        );
        assert_eq!(out[0].due.as_deref(), Some("até sexta"));
        let (system, user) = fake.single_call();
        assert!(system.contains("Só ações de \"Eu\""));
        assert!(user.contains("\"Eu\" é Marcus"));
        assert!(user.contains("[12:34] Ana: Marcus, você me manda"));
        assert!(user.contains("[00:05] Participante 1:"));
    }

    #[test]
    fn minuto_inventado_fica_sem_minuto() {
        let v = serde_json::json!({"actions":[
            {"title":"Revisar o contrato","at":"45:00","due":null,"quote":"x"},
            {"title":"Ligar pro João","at":"sei lá","due":null,"quote":""}
        ]});
        let out = parse_actions(&v, &reuniao()).unwrap();
        assert_eq!(out[0].at_secs, None);
        assert_eq!(out[1].at_secs, None);
        assert_eq!(out[1].quote, None);
    }

    #[test]
    fn relogio_vai_e_volta() {
        assert_eq!(clock(754.6), "12:34");
        assert_eq!(clock(3730.0), "1:02:10");
        assert_eq!(parse_clock("12:34"), Some(754.0));
        assert_eq!(parse_clock("[1:02:10]"), Some(3730.0));
        assert_eq!(parse_clock("12m34s"), Some(754.0));
        assert_eq!(parse_clock("12:75"), None);
    }

    #[test]
    fn reuniao_vazia_nao_chama_a_api() {
        let fake = FakeProvider::replying("{}");
        assert!(extract_my_actions(&fake, &[], None).unwrap().is_empty());
        assert!(
            extract_my_actions(&fake, &[line(1.0, "Eu", "  ")], None)
                .unwrap()
                .is_empty()
        );
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn reuniao_longa_vai_em_pedacos_com_sobreposicao_e_sem_repetir() {
        let fala = "a".repeat(900);
        let lines: Vec<TimedLine> = (0..120)
            .map(|i| {
                line(
                    i as f32 * 30.0,
                    if i % 2 == 0 { "Eu" } else { "Ana" },
                    &fala,
                )
            })
            .collect();
        let pedacos = chunks(&lines);
        assert!(pedacos.len() >= 3, "{} pedaços", pedacos.len());
        assert!(pedacos.iter().all(|p| p.len() <= CHUNK_CHARS + 1000));
        // A última fala de um pedaço volta no começo do seguinte.
        let ultima = pedacos[0].lines().last().unwrap();
        assert!(pedacos[1].contains(ultima));

        let fake = FakeProvider::replying(
            r#"{"actions":[{"title":"Mandar a planilha para a Ana","at":"00:30","due":null,"quote":"q"}]}"#,
        );
        let out = extract_my_actions(&fake, &lines, None).unwrap();
        assert_eq!(fake.calls().len(), pedacos.len());
        assert_eq!(
            out.len(),
            1,
            "a mesma ação vista em vários pedaços vira uma"
        );
    }

    #[test]
    fn repetidas_juntam_e_ordenam_pelo_minuto() {
        let a = |t: &str, at: Option<f32>, due: Option<&str>| MyAction {
            title: t.into(),
            at_secs: at,
            due: due.map(String::from),
            quote: None,
        };
        let out = merge(vec![
            a("Revisar o contrato da Marca", Some(900.0), None),
            a("Mandar a planilha de estimativas", Some(120.0), None),
            a(
                "mandar a planilha de estimativas pra Ana",
                None,
                Some("até sexta"),
            ),
        ]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].title, "Mandar a planilha de estimativas");
        assert_eq!(out[0].due.as_deref(), Some("até sexta"));
        assert!(similarity("Ligar pro João", "Revisar o PR") < 0.2);
    }
}
