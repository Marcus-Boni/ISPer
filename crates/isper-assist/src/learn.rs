//! Aprender é sugerir (Fase 10.5, [ADR 0023]): a mesma tarefa em pelo menos
//! 3 dos últimos 4 dias iguais da semana vira uma sugestão de rotina, com os
//! dias como evidência. Nada aqui grava; aceitar e recusar ficam no
//! [`AssistStore`](crate::AssistStore), e só com o toque da pessoa.
//!
//! - **A janela** são as últimas 4 semanas até hoje: cada dia da semana
//!   aparece 4 vezes nela.
//! - **A mesma tarefa** é título parecido (Jaccard das palavras ≥ 0,6), fora
//!   as tarefas de rotina e as descartadas.
//! - **O dia de uma tarefa** é o planejado; sem ele, o da conclusão.
//! - **Dias úteis:** com 3 ou mais dias úteis no padrão e 15 das 20
//!   ocorrências possíveis de segunda a sexta, a sugestão vira "dias úteis",
//!   em vez de deixar de fora o dia de um feriado.
//! - **Hora:** com 3 ocorrências com hora, todas a até 30 min da mediana, a
//!   rotina leva a mediana (arredondada a 5 min).
//! - **Recusar ajusta:** a mesma sugestão (título parecido e os mesmos dias)
//!   fica calada por 8 semanas e depois só volta com 4 de 4. Com mais de duas
//!   recusas para cada aceite (e ao menos 3 recusas), toda sugestão passa a
//!   pedir 4 de 4.
//! - **Aceitar ajusta:** a rotina criada tira a tarefa das sugestões, como
//!   qualquer rotina de título parecido, ligada ou pausada.
//!
//! [ADR 0023]: ../../../docs/adr/0023-escada-de-confianca.md

use chrono::{Datelike, Duration, NaiveDate, NaiveTime, Timelike, Weekday};
use serde::{Deserialize, Serialize};

use crate::recur::{ByDay, Freq, Rule};
use crate::text::{similarity, words};

/// Quantas semanas a janela olha.
pub const WEEKS: usize = 4;
/// Quantas das 4 semanas bastam.
pub const MIN_HITS: usize = 3;
/// Quanto uma sugestão recusada fica calada.
pub const MUTE_DAYS: i64 = 56;
/// A partir de que semelhança dois títulos são a mesma tarefa.
const SAME_TASK: f32 = 0.6;
/// Quantas sugestões de uma vez, no máximo.
const MAX_SUGGESTIONS: usize = 3;
/// Quanto as horas podem se afastar da mediana (min).
const TIME_SPREAD_MIN: i64 = 30;

const WORKDAYS: [Weekday; 5] = [
    Weekday::Mon,
    Weekday::Tue,
    Weekday::Wed,
    Weekday::Thu,
    Weekday::Fri,
];

/// Uma ocorrência de uma tarefa: o título e o dia.
#[derive(Debug, Clone, PartialEq)]
pub struct Seen {
    /// A tarefa.
    pub task_id: String,
    /// O título dela.
    pub title: String,
    /// O dia em que contou.
    pub day: NaiveDate,
    /// A hora marcada, `HH:MM`.
    pub time: Option<String>,
}

/// Uma decisão anterior sobre uma sugestão (vem do diário).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    /// Aceita (virou rotina) ou recusada.
    pub accepted: bool,
    /// Quando.
    pub day: NaiveDate,
    /// O título sugerido.
    pub title: String,
    /// Os dias da semana sugeridos (`MO`, `TU`…).
    pub weekdays: Vec<String>,
}

/// Uma sugestão de rotina, com a evidência.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RoutineSuggestion {
    /// Identifica a sugestão para aceitar ou recusar (as palavras do título e
    /// os dias da semana).
    pub key: String,
    /// O título da rotina (o da ocorrência mais recente).
    pub title: String,
    /// A recorrência, na forma canônica do RRULE.
    pub rrule: String,
    /// A mesma recorrência, aberta para a interface.
    pub rule: Rule,
    /// Os dias da semana (`MO`, `TU`…).
    pub weekdays: Vec<String>,
    /// Em quantos dos dias possíveis a tarefa apareceu.
    pub hits: usize,
    /// Quantos dias possíveis (4 por dia da semana).
    pub of: usize,
    /// Os dias em que apareceu, do mais antigo ao mais novo.
    pub days: Vec<NaiveDate>,
    /// As tarefas desses dias.
    pub task_ids: Vec<String>,
}

fn code(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "MO",
        Weekday::Tue => "TU",
        Weekday::Wed => "WE",
        Weekday::Thu => "TH",
        Weekday::Fri => "FR",
        Weekday::Sat => "SA",
        Weekday::Sun => "SU",
    }
}

/// O primeiro dia da janela que termina em `today`.
pub fn window_start(today: NaiveDate) -> NaiveDate {
    today - Duration::days((7 * WEEKS) as i64 - 1)
}

/// As sugestões para hoje. `routines` são os títulos das rotinas que já
/// existem (ligadas ou pausadas).
pub fn suggest(
    seen: &[Seen],
    routines: &[String],
    decisions: &[Decision],
    today: NaiveDate,
) -> Vec<RoutineSuggestion> {
    let start = window_start(today);
    let declined = decisions.iter().filter(|d| !d.accepted).count();
    let accepted = decisions.len() - declined;
    let strict = declined >= 3 && declined > 2 * accepted;

    let mut recent: Vec<&Seen> = seen
        .iter()
        .filter(|s| s.day >= start && s.day <= today)
        .collect();
    recent.sort_by(|a, b| a.day.cmp(&b.day).then(a.task_id.cmp(&b.task_id)));

    // Junta os títulos parecidos; o grupo compara com o título mais recente.
    let mut groups: Vec<Vec<&Seen>> = Vec::new();
    for s in recent.into_iter().rev() {
        match groups
            .iter_mut()
            .find(|g| similarity(&g[0].title, &s.title) >= SAME_TASK)
        {
            Some(g) => g.push(s),
            None => groups.push(vec![s]),
        }
    }

    let mut out = Vec::new();
    for group in groups {
        let title = group[0].title.clone();
        if routines.iter().any(|r| similarity(r, &title) >= SAME_TASK) {
            continue;
        }
        let mut days: Vec<NaiveDate> = group.iter().map(|s| s.day).collect();
        days.sort();
        days.dedup();
        let count = |w: Weekday| days.iter().filter(|d| d.weekday() == w).count();
        let weekdays = pattern_days(&count, if strict { WEEKS } else { MIN_HITS });
        if weekdays.is_empty() {
            continue;
        }
        let codes: Vec<String> = weekdays.iter().map(|w| code(*w).to_string()).collect();
        // Recusada antes: calada por 8 semanas, depois só com 4 de 4.
        let refused = decisions.iter().rev().find(|d| {
            !d.accepted && d.weekdays == codes && similarity(&d.title, &title) >= SAME_TASK
        });
        if let Some(d) = refused {
            let quiet = (today - d.day).num_days() < MUTE_DAYS;
            let full = weekdays.iter().all(|w| count(*w) == WEEKS);
            if quiet || !full {
                continue;
            }
        }
        let on_pattern: Vec<&&Seen> = group
            .iter()
            .filter(|s| weekdays.contains(&s.day.weekday()))
            .collect();
        let mut evidence: Vec<NaiveDate> = on_pattern.iter().map(|s| s.day).collect();
        evidence.sort();
        evidence.dedup();
        let mut task_ids: Vec<String> = on_pattern.iter().map(|s| s.task_id.clone()).collect();
        task_ids.sort();
        task_ids.dedup();
        let rule = rule_for(&weekdays, typical_time(&on_pattern));
        out.push(RoutineSuggestion {
            key: format!(
                "{}|{}",
                words(&title).into_iter().collect::<Vec<_>>().join("-"),
                codes.join(",")
            ),
            title,
            rrule: rule.to_string(),
            rule,
            weekdays: codes,
            hits: evidence.len(),
            of: weekdays.len() * WEEKS,
            days: evidence,
            task_ids,
        });
    }
    out.sort_by(|a, b| {
        let ra = a.hits as f32 / a.of as f32;
        let rb = b.hits as f32 / b.of as f32;
        rb.total_cmp(&ra)
            .then(b.days.last().cmp(&a.days.last()))
            .then(a.key.cmp(&b.key))
    });
    out.truncate(MAX_SUGGESTIONS);
    out
}

/// Os dias da semana do padrão, de segunda a domingo.
fn pattern_days(count: &dyn Fn(Weekday) -> usize, need: usize) -> Vec<Weekday> {
    let all = [
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
        Weekday::Sat,
        Weekday::Sun,
    ];
    let mut days: Vec<Weekday> = all.into_iter().filter(|w| count(*w) >= need).collect();
    let workdays_in = days.iter().filter(|w| WORKDAYS.contains(w)).count();
    let workday_hits: usize = WORKDAYS.iter().map(|w| count(*w)).sum();
    // Um feriado não tira um dia útil de um padrão de dias úteis.
    if workdays_in >= 3 && workday_hits * 4 >= WORKDAYS.len() * WEEKS * 3 {
        for w in WORKDAYS {
            if !days.contains(&w) {
                days.push(w);
            }
        }
        days.sort_by_key(|w| w.num_days_from_monday());
    }
    days
}

/// A hora de costume: a mediana, se todas as horas ficam perto dela.
fn typical_time(seen: &[&&Seen]) -> Option<NaiveTime> {
    let mut minutes: Vec<i64> = seen
        .iter()
        .filter_map(|s| s.time.as_deref())
        .filter_map(|t| NaiveTime::parse_from_str(t, "%H:%M").ok())
        .map(|t| i64::from(t.hour()) * 60 + i64::from(t.minute()))
        .collect();
    if minutes.len() < MIN_HITS {
        return None;
    }
    minutes.sort();
    let median = minutes[minutes.len() / 2];
    if minutes.iter().any(|m| (m - median).abs() > TIME_SPREAD_MIN) {
        return None;
    }
    let rounded = ((median + 2) / 5 * 5).min(23 * 60 + 55);
    NaiveTime::from_hms_opt((rounded / 60) as u32, (rounded % 60) as u32, 0)
}

fn rule_for(weekdays: &[Weekday], time: Option<NaiveTime>) -> Rule {
    let daily = weekdays.len() == 7;
    Rule {
        freq: if daily { Freq::Daily } else { Freq::Weekly },
        interval: 1,
        by_day: if daily {
            Vec::new()
        } else {
            weekdays
                .iter()
                .map(|w| ByDay {
                    nth: None,
                    weekday: *w,
                })
                .collect()
        },
        by_month_day: Vec::new(),
        time,
        until: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn seen(id: &str, title: &str, day: &str, time: Option<&str>) -> Seen {
        Seen {
            task_id: id.into(),
            title: title.into(),
            day: d(day),
            time: time.map(String::from),
        }
    }

    // Quinta, 08/10/2026: a janela começa na sexta 11/09, e as segundas
    // nela são 14/09, 21/09, 28/09 e 05/10.
    const TODAY: &str = "2026-10-08";

    #[test]
    fn tres_de_quatro_segundas_vira_rotina_semanal_com_a_hora() {
        let s = vec![
            seen("a", "Revisar os PRs abertos", "2026-09-14", Some("09:00")),
            seen("b", "Revisar PRs", "2026-09-21", Some("09:10")),
            seen("c", "revisar os PRs", "2026-10-05", Some("08:55")),
            seen("x", "Pagar o boleto", "2026-10-05", None),
        ];
        let out = suggest(&s, &[], &[], d(TODAY));
        assert_eq!(out.len(), 1);
        let r = &out[0];
        assert_eq!(r.title, "revisar os PRs", "o título mais recente");
        assert_eq!(r.weekdays, ["MO"]);
        assert_eq!((r.hits, r.of), (3, 4));
        assert_eq!(r.days, [d("2026-09-14"), d("2026-09-21"), d("2026-10-05")]);
        assert_eq!(r.rrule, "FREQ=WEEKLY;BYDAY=MO;BYHOUR=9;BYMINUTE=0");
        assert_eq!(
            Rule::parse(&r.rrule).unwrap(),
            r.rule,
            "a regra volta igual"
        );
        assert_eq!(r.key, "prs-revisar|MO");
    }

    #[test]
    fn duas_de_quatro_nao_bastam_e_horas_espalhadas_ficam_sem_hora() {
        let s = vec![
            seen("a", "Ler o resumo", "2026-09-21", None),
            seen("b", "Ler o resumo", "2026-10-05", None),
        ];
        assert!(suggest(&s, &[], &[], d(TODAY)).is_empty());
        let s = vec![
            seen("a", "Ler o resumo", "2026-09-14", Some("08:00")),
            seen("b", "Ler o resumo", "2026-09-21", Some("11:00")),
            seen("c", "Ler o resumo", "2026-10-05", Some("09:00")),
        ];
        let out = suggest(&s, &[], &[], d(TODAY));
        assert_eq!(out[0].rule.time, None);
    }

    #[test]
    fn dias_uteis_com_um_feriado_no_meio() {
        // Segunda a sexta nas 4 semanas, menos 3 dias: as terças ficam com 2
        // de 4, mas 17 das 20 ocorrências de dia útil seguram o padrão.
        let mut s = Vec::new();
        let mut day = window_start(d(TODAY));
        let skip = [d("2026-09-15"), d("2026-09-22"), d("2026-10-01")];
        let mut n = 0;
        while day <= d(TODAY) {
            if WORKDAYS.contains(&day.weekday()) && !skip.contains(&day) {
                s.push(seen(
                    &format!("t{n}"),
                    "Conferir o e-mail do cliente",
                    &day.to_string(),
                    None,
                ));
                n += 1;
            }
            day += Duration::days(1);
        }
        let out = suggest(&s, &[], &[], d(TODAY));
        assert_eq!(out[0].weekdays, ["MO", "TU", "WE", "TH", "FR"]);
        assert_eq!(out[0].rrule, "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR");
    }

    #[test]
    fn todo_dia_vira_diaria() {
        let mut s = Vec::new();
        let mut day = window_start(d(TODAY));
        let mut n = 0;
        while day <= d(TODAY) {
            s.push(seen(
                &format!("t{n}"),
                "Tomar o remédio",
                &day.to_string(),
                None,
            ));
            n += 1;
            day += Duration::days(1);
        }
        let out = suggest(&s, &[], &[], d(TODAY));
        assert_eq!(out[0].rrule, "FREQ=DAILY");
        assert_eq!((out[0].hits, out[0].of), (28, 28));
    }

    #[test]
    fn rotina_que_ja_existe_nao_volta_como_sugestao() {
        let s = vec![
            seen("a", "Revisar os PRs", "2026-09-14", None),
            seen("b", "Revisar os PRs", "2026-09-21", None),
            seen("c", "Revisar os PRs", "2026-10-05", None),
        ];
        assert!(suggest(&s, &["Revisar PRs abertos".into()], &[], d(TODAY)).is_empty());
    }

    #[test]
    fn recusada_fica_calada_e_so_volta_com_quatro_de_quatro() {
        let three = vec![
            seen("a", "Revisar os PRs", "2026-09-14", None),
            seen("b", "Revisar os PRs", "2026-09-21", None),
            seen("c", "Revisar os PRs", "2026-10-05", None),
        ];
        let refused = |day: &str| Decision {
            accepted: false,
            day: d(day),
            title: "Revisar PRs".into(),
            weekdays: vec!["MO".into()],
        };
        assert!(suggest(&three, &[], &[refused("2026-10-01")], d(TODAY)).is_empty());
        // Passadas as 8 semanas, 3 de 4 ainda não bastam…
        assert!(suggest(&three, &[], &[refused("2026-08-01")], d(TODAY)).is_empty());
        // …e 4 de 4 trazem de volta.
        let mut four = three.clone();
        four.push(seen("d", "Revisar os PRs", "2026-09-28", None));
        assert_eq!(
            suggest(&four, &[], &[refused("2026-08-01")], d(TODAY)).len(),
            1
        );
        // Outros dias da semana são outra sugestão.
        let other = Decision {
            weekdays: vec!["TU".into()],
            ..refused("2026-10-01")
        };
        assert_eq!(suggest(&three, &[], &[other], d(TODAY)).len(), 1);
    }

    #[test]
    fn muitas_recusas_pedem_quatro_de_quatro_para_tudo() {
        let three = vec![
            seen("a", "Revisar os PRs", "2026-09-14", None),
            seen("b", "Revisar os PRs", "2026-09-21", None),
            seen("c", "Revisar os PRs", "2026-10-05", None),
        ];
        let no = |t: &str| Decision {
            accepted: false,
            day: d("2026-09-01"),
            title: t.into(),
            weekdays: vec!["FR".into()],
        };
        let decisions = [no("Um"), no("Dois"), no("Três")];
        assert!(suggest(&three, &[], &decisions, d(TODAY)).is_empty());
        let yes = Decision {
            accepted: true,
            ..no("Quatro")
        };
        let decisions = [no("Um"), no("Dois"), no("Três"), yes.clone(), yes];
        assert_eq!(suggest(&three, &[], &decisions, d(TODAY)).len(), 1);
    }

    #[test]
    fn fora_da_janela_nao_conta() {
        let s = vec![
            seen("a", "Revisar os PRs", "2026-09-07", None),
            seen("b", "Revisar os PRs", "2026-09-21", None),
            seen("c", "Revisar os PRs", "2026-10-05", None),
        ];
        assert!(suggest(&s, &[], &[], d(TODAY)).is_empty());
    }
}
