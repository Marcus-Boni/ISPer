//! Recorrência das rotinas: o pedaço do RRULE (RFC 5545) que cobre o que uma
//! pessoa repete no trabalho.
//!
//! - `FREQ=DAILY`, `WEEKLY` ou `MONTHLY`, com `INTERVAL`;
//! - `BYDAY` (`MO`…`SU`; no mensal, com ordinal: `1MO` é a primeira
//!   segunda, `-1FR` a última sexta);
//! - `BYMONTHDAY` no mensal (`-1` é o último dia do mês);
//! - `BYHOUR` e `BYMINUTE` com um valor só: a hora do dia da rotina;
//! - `UNTIL`, até quando vale.
//!
//! O resto (`COUNT`, `BYSETPOS`, `YEARLY`…) é recusado em vez de ignorado: uma
//! regra lida pela metade marcaria a rotina no dia errado sem ninguém notar.
//! O começo da contagem do `INTERVAL` é o dia em que a rotina nasceu, que o
//! chamador passa como `anchor`.

use std::fmt;

use chrono::{Datelike, Days, NaiveDate, NaiveTime, Timelike, Weekday};
use serde::{Serialize, Serializer};

use crate::{AssistError, Result};

/// Quanto à frente [`Rule::next_on_or_after`] procura (dez anos e pouco).
const SEARCH_DAYS: u64 = 3_700;

/// De quanto em quanto a rotina volta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Freq {
    /// Todo dia (ou a cada `INTERVAL` dias).
    Daily,
    /// Toda semana, nos dias de `BYDAY`.
    Weekly,
    /// Todo mês, nos dias de `BYMONTHDAY` ou `BYDAY`.
    Monthly,
}

impl Freq {
    fn as_str(self) -> &'static str {
        match self {
            Self::Daily => "DAILY",
            Self::Weekly => "WEEKLY",
            Self::Monthly => "MONTHLY",
        }
    }
}

/// Um dia da semana, com ordinal opcional no mensal (`1MO`, `-1FR`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByDay {
    /// `Some(1)` é o primeiro do mês, `Some(-1)` o último; `None`, todos.
    pub nth: Option<i8>,
    /// O dia da semana.
    pub weekday: Weekday,
}

impl fmt::Display for ByDay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(n) = self.nth {
            write!(f, "{n}")?;
        }
        f.write_str(weekday_code(self.weekday))
    }
}

impl Serialize for ByDay {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

/// Uma regra de recorrência já validada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rule {
    /// A frequência.
    pub freq: Freq,
    /// A cada quantas unidades da frequência (1 = toda).
    pub interval: u32,
    /// Dias da semana.
    pub by_day: Vec<ByDay>,
    /// Dias do mês (negativos contam do fim).
    pub by_month_day: Vec<i8>,
    /// A hora do dia, `HH:MM`.
    #[serde(serialize_with = "time_hhmm")]
    pub time: Option<NaiveTime>,
    /// Último dia em que vale.
    pub until: Option<NaiveDate>,
}

fn time_hhmm<S: Serializer>(t: &Option<NaiveTime>, s: S) -> std::result::Result<S::Ok, S::Error> {
    match t {
        Some(t) => s.collect_str(&t.format("%H:%M")),
        None => s.serialize_none(),
    }
}

impl Rule {
    /// Lê uma regra (`FREQ=WEEKLY;BYDAY=MO,TU;BYHOUR=17`), com ou sem o
    /// prefixo `RRULE:`, sem diferença entre maiúsculas e minúsculas.
    pub fn parse(text: &str) -> Result<Self> {
        let text = text.trim();
        let body = text
            .get(..6)
            .filter(|p| p.eq_ignore_ascii_case("RRULE:"))
            .map_or(text, |_| &text[6..]);
        let mut freq = None;
        let mut interval = None;
        let mut by_day = None;
        let mut by_month_day = None;
        let mut hour = None;
        let mut minute = None;
        let mut until = None;
        for part in body.split(';').map(str::trim).filter(|p| !p.is_empty()) {
            let (key, value) = part
                .split_once('=')
                .ok_or_else(|| invalid(format!("parte sem '=': {part}")))?;
            let key = key.trim().to_ascii_uppercase();
            let value = value.trim().to_ascii_uppercase();
            let dup = || invalid(format!("{key} repetido"));
            match key.as_str() {
                "FREQ" => {
                    let f = match value.as_str() {
                        "DAILY" => Freq::Daily,
                        "WEEKLY" => Freq::Weekly,
                        "MONTHLY" => Freq::Monthly,
                        other => {
                            return Err(invalid(format!(
                                "frequência {other} não suportada (use DAILY, WEEKLY ou MONTHLY)"
                            )));
                        }
                    };
                    if freq.replace(f).is_some() {
                        return Err(dup());
                    }
                }
                "INTERVAL" => {
                    let n: u32 = value
                        .parse()
                        .ok()
                        .filter(|n| (1..=366).contains(n))
                        .ok_or_else(|| invalid(format!("INTERVAL inválido: {value}")))?;
                    if interval.replace(n).is_some() {
                        return Err(dup());
                    }
                }
                "BYDAY" => {
                    let days = value
                        .split(',')
                        .map(parse_by_day)
                        .collect::<Result<Vec<_>>>()?;
                    if by_day.replace(days).is_some() {
                        return Err(dup());
                    }
                }
                "BYMONTHDAY" => {
                    let days = value
                        .split(',')
                        .map(|d| {
                            d.trim()
                                .parse::<i8>()
                                .ok()
                                .filter(|n| *n != 0 && (-31..=31).contains(n))
                                .ok_or_else(|| invalid(format!("BYMONTHDAY inválido: {d}")))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    if by_month_day.replace(days).is_some() {
                        return Err(dup());
                    }
                }
                "BYHOUR" => {
                    let h: u32 = value.parse().ok().filter(|h| *h < 24).ok_or_else(|| {
                        invalid(format!("BYHOUR inválido: {value} (uma hora só, 0 a 23)"))
                    })?;
                    if hour.replace(h).is_some() {
                        return Err(dup());
                    }
                }
                "BYMINUTE" => {
                    let m: u32 = value.parse().ok().filter(|m| *m < 60).ok_or_else(|| {
                        invalid(format!("BYMINUTE inválido: {value} (um minuto só, 0 a 59)"))
                    })?;
                    if minute.replace(m).is_some() {
                        return Err(dup());
                    }
                }
                "UNTIL" => {
                    let date = value.get(..8).unwrap_or_default();
                    let d = NaiveDate::parse_from_str(date, "%Y%m%d")
                        .map_err(|_| invalid(format!("UNTIL inválido: {value} (use AAAAMMDD)")))?;
                    if until.replace(d).is_some() {
                        return Err(dup());
                    }
                }
                // A semana começa na segunda; outro começo mudaria o INTERVAL
                // semanal e não vale o código.
                "WKST" if value == "MO" => {}
                other => {
                    return Err(invalid(format!("{other} não é suportado nas rotinas")));
                }
            }
        }
        let freq = freq.ok_or_else(|| invalid("falta a frequência (FREQ)".into()))?;
        let by_day = by_day.unwrap_or_default();
        let by_month_day = by_month_day.unwrap_or_default();
        if freq != Freq::Monthly {
            if !by_month_day.is_empty() {
                return Err(invalid("BYMONTHDAY só vale na recorrência mensal".into()));
            }
            if by_day.iter().any(|d| d.nth.is_some()) {
                return Err(invalid(
                    "ordinal no BYDAY (1MO, -1FR) só vale na recorrência mensal".into(),
                ));
            }
        }
        let time = match (hour, minute) {
            (None, None) => None,
            (None, Some(_)) => return Err(invalid("BYMINUTE sem BYHOUR".into())),
            (Some(h), m) => NaiveTime::from_hms_opt(h, m.unwrap_or(0), 0),
        };
        Ok(Self {
            freq,
            interval: interval.unwrap_or(1),
            by_day: sorted(by_day, |d| (d.weekday.num_days_from_monday(), d.nth)),
            by_month_day: sorted(by_month_day, |d| *d),
            time,
            until,
        })
    }

    /// Dias úteis (segunda a sexta), numa hora.
    pub fn workdays_at(hour: u32, minute: u32) -> Self {
        Self {
            freq: Freq::Weekly,
            interval: 1,
            by_day: [
                Weekday::Mon,
                Weekday::Tue,
                Weekday::Wed,
                Weekday::Thu,
                Weekday::Fri,
            ]
            .into_iter()
            .map(|weekday| ByDay { nth: None, weekday })
            .collect(),
            by_month_day: Vec::new(),
            time: NaiveTime::from_hms_opt(hour, minute, 0),
            until: None,
        }
    }

    /// A hora da rotina em `HH:MM`, se tiver.
    pub fn time_hhmm(&self) -> Option<String> {
        self.time.map(|t| t.format("%H:%M").to_string())
    }

    /// Se a rotina cai em `day`, contando o `INTERVAL` a partir de `anchor`
    /// (o dia em que ela nasceu). Antes do `anchor`, nunca.
    pub fn occurs_on(&self, day: NaiveDate, anchor: NaiveDate) -> bool {
        if day < anchor || self.until.is_some_and(|u| day > u) {
            return false;
        }
        let interval = i64::from(self.interval.max(1));
        let weekday_listed = || self.by_day.iter().any(|d| d.weekday == day.weekday());
        match self.freq {
            Freq::Daily => {
                (day - anchor).num_days() % interval == 0
                    && (self.by_day.is_empty() || weekday_listed())
            }
            Freq::Weekly => {
                let weeks = (monday_of(day) - monday_of(anchor)).num_days() / 7;
                weeks % interval == 0
                    && if self.by_day.is_empty() {
                        day.weekday() == anchor.weekday()
                    } else {
                        weekday_listed()
                    }
            }
            Freq::Monthly => {
                let months = month_index(day) - month_index(anchor);
                months % interval == 0 && self.month_matches(day, anchor)
            }
        }
    }

    fn month_matches(&self, day: NaiveDate, anchor: NaiveDate) -> bool {
        if self.by_day.is_empty() && self.by_month_day.is_empty() {
            return day.day() == anchor.day();
        }
        let len = days_in_month(day);
        let dom = day.day() as i32;
        let by_month_day = self.by_month_day.is_empty()
            || self.by_month_day.iter().any(|&md| {
                let md = i32::from(md);
                if md > 0 {
                    dom == md
                } else {
                    dom == len + md + 1
                }
            });
        let by_day = self.by_day.is_empty()
            || self.by_day.iter().any(|d| {
                d.weekday == day.weekday()
                    && match d.nth {
                        None => true,
                        Some(n) if n > 0 => (dom - 1) / 7 + 1 == i32::from(n),
                        Some(n) => (len - dom) / 7 + 1 == -i32::from(n),
                    }
            });
        by_month_day && by_day
    }

    /// O primeiro dia a partir de `from` (inclusive) em que a rotina cai.
    /// `None` se acabou (`UNTIL`) ou não cai nos próximos dez anos.
    pub fn next_on_or_after(&self, from: NaiveDate, anchor: NaiveDate) -> Option<NaiveDate> {
        let start = from.max(anchor);
        (0..SEARCH_DAYS)
            .filter_map(|n| start.checked_add_days(Days::new(n)))
            .take_while(|d| self.until.is_none_or(|u| *d <= u))
            .find(|d| self.occurs_on(*d, anchor))
    }
}

/// A forma canônica, a que vai para o banco.
impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FREQ={}", self.freq.as_str())?;
        if self.interval > 1 {
            write!(f, ";INTERVAL={}", self.interval)?;
        }
        if !self.by_day.is_empty() {
            let days: Vec<String> = self.by_day.iter().map(ToString::to_string).collect();
            write!(f, ";BYDAY={}", days.join(","))?;
        }
        if !self.by_month_day.is_empty() {
            let days: Vec<String> = self.by_month_day.iter().map(ToString::to_string).collect();
            write!(f, ";BYMONTHDAY={}", days.join(","))?;
        }
        if let Some(t) = self.time {
            write!(f, ";BYHOUR={};BYMINUTE={}", t.hour(), t.minute())?;
        }
        if let Some(u) = self.until {
            write!(f, ";UNTIL={}", u.format("%Y%m%d"))?;
        }
        Ok(())
    }
}

fn invalid(msg: String) -> AssistError {
    AssistError::Invalid(format!("recorrência: {msg}"))
}

fn parse_by_day(text: &str) -> Result<ByDay> {
    let text = text.trim();
    let split = text.len().saturating_sub(2);
    if !text.is_char_boundary(split) {
        return Err(invalid(format!("dia da semana inválido: {text}")));
    }
    let (nth, code) = text.split_at(split);
    let weekday = match code {
        "MO" => Weekday::Mon,
        "TU" => Weekday::Tue,
        "WE" => Weekday::Wed,
        "TH" => Weekday::Thu,
        "FR" => Weekday::Fri,
        "SA" => Weekday::Sat,
        "SU" => Weekday::Sun,
        _ => return Err(invalid(format!("dia da semana inválido: {text}"))),
    };
    let nth = if nth.is_empty() {
        None
    } else {
        let n: i8 = nth
            .trim_start_matches('+')
            .parse()
            .ok()
            .filter(|n: &i8| *n != 0 && (-5..=5).contains(n))
            .ok_or_else(|| invalid(format!("ordinal inválido: {text} (de -5 a 5)")))?;
        Some(n)
    };
    Ok(ByDay { nth, weekday })
}

fn weekday_code(w: Weekday) -> &'static str {
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

/// Sem repetidos e numa ordem só, para a forma canônica não depender de como
/// a regra foi escrita.
fn sorted<T: PartialEq, K: Ord>(mut items: Vec<T>, key: impl Fn(&T) -> K) -> Vec<T> {
    items.sort_by_key(|i| key(i));
    items.dedup();
    items
}

fn monday_of(day: NaiveDate) -> NaiveDate {
    day - chrono::Duration::days(i64::from(day.weekday().num_days_from_monday()))
}

fn month_index(day: NaiveDate) -> i64 {
    i64::from(day.year()) * 12 + i64::from(day.month0())
}

fn days_in_month(day: NaiveDate) -> i32 {
    let first = day.with_day(1).unwrap_or(day);
    let next = first
        .checked_add_months(chrono::Months::new(1))
        .unwrap_or(first);
    (next - first).num_days() as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    /// Os dias em que a regra cai num intervalo, a partir do anchor.
    fn days_in(rule: &str, anchor: &str, from: &str, to: &str) -> Vec<String> {
        let rule = Rule::parse(rule).unwrap();
        let anchor = day(anchor);
        day(from)
            .iter_days()
            .take_while(|d| *d <= day(to))
            .filter(|d| rule.occurs_on(*d, anchor))
            .map(|d| d.to_string())
            .collect()
    }

    #[test]
    fn dias_uteis_as_17() {
        let rule = Rule::parse("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR;BYHOUR=17;BYMINUTE=0").unwrap();
        assert_eq!(rule, Rule::workdays_at(17, 0));
        assert_eq!(rule.time_hhmm().as_deref(), Some("17:00"));
        // Semana de 05/10/2026: segunda a domingo.
        assert_eq!(
            days_in(&rule.to_string(), "2026-10-01", "2026-10-05", "2026-10-11"),
            [
                "2026-10-05",
                "2026-10-06",
                "2026-10-07",
                "2026-10-08",
                "2026-10-09"
            ]
        );
    }

    #[test]
    fn diaria_com_intervalo_conta_do_nascimento() {
        assert_eq!(
            days_in(
                "FREQ=DAILY;INTERVAL=3",
                "2026-10-07",
                "2026-10-01",
                "2026-10-16"
            ),
            ["2026-10-07", "2026-10-10", "2026-10-13", "2026-10-16"],
            "nada antes de nascer, e de três em três dias depois"
        );
    }

    #[test]
    fn semanal_sem_dia_usa_o_dia_do_nascimento_e_quinzenal_pula_semana() {
        // 07/10/2026 é quarta.
        assert_eq!(
            days_in("FREQ=WEEKLY", "2026-10-07", "2026-10-07", "2026-10-28"),
            ["2026-10-07", "2026-10-14", "2026-10-21", "2026-10-28"]
        );
        // Quinzenal às segundas e sextas: a semana conta de segunda a domingo,
        // então a sexta da semana do nascimento já vale.
        assert_eq!(
            days_in(
                "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,FR",
                "2026-10-07",
                "2026-10-05",
                "2026-10-25"
            ),
            ["2026-10-09", "2026-10-19", "2026-10-23"]
        );
    }

    #[test]
    fn mensal_por_dia_do_mes_e_ultimo_dia() {
        assert_eq!(
            days_in(
                "FREQ=MONTHLY;BYMONTHDAY=-1",
                "2026-01-01",
                "2026-01-01",
                "2026-04-30"
            ),
            ["2026-01-31", "2026-02-28", "2026-03-31", "2026-04-30"]
        );
        assert_eq!(
            days_in(
                "FREQ=MONTHLY;BYMONTHDAY=31",
                "2026-01-01",
                "2026-01-01",
                "2026-04-30"
            ),
            ["2026-01-31", "2026-03-31"],
            "mês sem dia 31 fica sem a rotina, como no RFC"
        );
        assert_eq!(
            days_in("FREQ=MONTHLY", "2026-01-15", "2026-01-01", "2026-03-31"),
            ["2026-01-15", "2026-02-15", "2026-03-15"],
            "sem dia, o do nascimento"
        );
    }

    #[test]
    fn mensal_por_ordinal() {
        // Última sexta: fechar a folha.
        assert_eq!(
            days_in(
                "FREQ=MONTHLY;BYDAY=-1FR",
                "2026-09-01",
                "2026-09-01",
                "2026-12-31"
            ),
            ["2026-09-25", "2026-10-30", "2026-11-27", "2026-12-25"]
        );
        // Primeira segunda, de dois em dois meses.
        assert_eq!(
            days_in(
                "FREQ=MONTHLY;INTERVAL=2;BYDAY=1MO",
                "2026-09-01",
                "2026-09-01",
                "2026-12-31"
            ),
            ["2026-09-07", "2026-11-02"]
        );
    }

    #[test]
    fn until_encerra() {
        assert_eq!(
            days_in(
                "FREQ=DAILY;UNTIL=20261009T235959Z",
                "2026-10-07",
                "2026-10-07",
                "2026-10-12"
            ),
            ["2026-10-07", "2026-10-08", "2026-10-09"]
        );
        let rule = Rule::parse("FREQ=DAILY;UNTIL=20261009").unwrap();
        assert_eq!(
            rule.next_on_or_after(day("2026-10-10"), day("2026-10-01")),
            None
        );
    }

    #[test]
    fn forma_canonica_e_tolerante_na_entrada() {
        let rule = Rule::parse(" rrule:freq=weekly; byday=fr,mo,fr ;byhour=9 ").unwrap();
        assert_eq!(
            rule.to_string(),
            "FREQ=WEEKLY;BYDAY=MO,FR;BYHOUR=9;BYMINUTE=0"
        );
        assert_eq!(Rule::parse(&rule.to_string()).unwrap(), rule);
        assert_eq!(
            Rule::parse("FREQ=DAILY;INTERVAL=1").unwrap().to_string(),
            "FREQ=DAILY"
        );
    }

    #[test]
    fn o_que_nao_e_suportado_e_recusado() {
        for bad in [
            "",
            "BYDAY=MO",
            "FREQ=YEARLY",
            "FREQ=DAILY;COUNT=3",
            "FREQ=MONTHLY;BYSETPOS=-1",
            "FREQ=WEEKLY;BYDAY=1MO",
            "FREQ=WEEKLY;BYMONTHDAY=1",
            "FREQ=DAILY;BYHOUR=9,17",
            "FREQ=DAILY;BYMINUTE=30",
            "FREQ=DAILY;BYHOUR=24",
            "FREQ=DAILY;INTERVAL=0",
            "FREQ=DAILY;FREQ=WEEKLY",
            "FREQ=MONTHLY;BYMONTHDAY=0",
            "FREQ=MONTHLY;BYDAY=6MO",
            "FREQ=WEEKLY;BYDAY=XX",
            "FREQ=WEEKLY;WKST=SU",
            "FREQ=DAILY;UNTIL=amanha",
            "FREQ=DAILY;oops",
        ] {
            assert!(Rule::parse(bad).is_err(), "deveria recusar {bad:?}");
        }
    }

    #[test]
    fn vista_para_a_interface() {
        let rule = Rule::parse("FREQ=MONTHLY;BYDAY=-1FR;BYHOUR=16;BYMINUTE=30").unwrap();
        let v = serde_json::to_value(&rule).unwrap();
        assert_eq!(v["freq"], "monthly");
        assert_eq!(v["by_day"][0], "-1FR");
        assert_eq!(v["time"], "16:30");
    }

    fn any_rule() -> impl Strategy<Value = String> {
        let wd = prop::sample::select(vec!["MO", "TU", "WE", "TH", "FR", "SA", "SU"]);
        prop_oneof![
            (1u32..5).prop_map(|i| format!("FREQ=DAILY;INTERVAL={i}")),
            (1u32..4, prop::collection::vec(wd.clone(), 0..4)).prop_map(|(i, d)| {
                if d.is_empty() {
                    format!("FREQ=WEEKLY;INTERVAL={i}")
                } else {
                    format!("FREQ=WEEKLY;INTERVAL={i};BYDAY={}", d.join(","))
                }
            }),
            (1u32..4, -3i8..29).prop_map(|(i, d)| {
                let d = if d == 0 { 1 } else { d };
                format!("FREQ=MONTHLY;INTERVAL={i};BYMONTHDAY={d}")
            }),
            (1u32..3, prop::sample::select(vec![-1i8, 1, 2, 3]), wd)
                .prop_map(|(i, n, d)| format!("FREQ=MONTHLY;INTERVAL={i};BYDAY={n}{d}")),
        ]
    }

    proptest! {
        #[test]
        fn proximo_dia_e_o_primeiro_em_que_cai(
            rule in any_rule(),
            anchor_off in 0u64..800,
            from_off in 0u64..800,
        ) {
            let base = day("2025-01-01");
            let anchor = base + Days::new(anchor_off);
            let from = base + Days::new(from_off);
            let rule = Rule::parse(&rule).unwrap();
            prop_assert_eq!(Rule::parse(&rule.to_string()).unwrap(), rule.clone());
            let next = rule.next_on_or_after(from, anchor);
            let next = next.expect("toda regra destas volta em menos de dez anos");
            prop_assert!(next >= from && next >= anchor);
            prop_assert!(rule.occurs_on(next, anchor));
            let start = from.max(anchor);
            for d in start.iter_days().take_while(|d| *d < next) {
                prop_assert!(!rule.occurs_on(d, anchor), "pulou {}", d);
            }
        }
    }
}
