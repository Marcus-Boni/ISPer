//! Datas e horas em português, sem IA (Fase 10.1).
//!
//! "amanhã às 3 ligar pro João" vira a tarefa "Ligar pro João" para amanhã às
//! 15:00. Isto é código, e não a LLM, de propósito: conta de dia da semana é
//! onde os modelos erram, e aqui cada regra tem teste. A LLM da captura por voz
//! devolve só o trecho falado ("sexta que vem"); quem vira data é [`resolve`].
//!
//! O que entende (sem acento e sem caixa: "AMANHÃ", "amanha" e "Amanhã" valem):
//!
//! - dias: hoje, amanhã, depois de amanhã; segunda a domingo (com "-feira",
//!   "na", "essa", "próxima", "que vem"); semana que vem; fim de semana; fim do
//!   mês; dia 12; 12/10 e 12/10/2026; 12 de outubro; daqui a 3 dias, em 2
//!   semanas;
//! - horas: 15:30, 15h30, 15h, às 3, às 3 e meia, às 7 da noite, meio-dia,
//!   meia-noite;
//! - prazo: "até sexta", "prazo dia 15" vão para o prazo, não para o dia.
//!
//! Regras de ambiguidade, que os cartões de revisão deixam corrigir:
//!
//! - "às 3" sem "da manhã" é 15:00: entre 1 e 6, a hora falada de uma tarefa de
//!   trabalho é da tarde. "03:00" com zero na frente fica de madrugada;
//! - "sexta" é a próxima sexta (hoje sendo sexta, a da semana que vem);
//!   "sexta que vem" e "próxima sexta" pulam uma semana quando a próxima sexta
//!   ainda é desta semana;
//! - um dia da semana solto no meio da frase só vale com "na", "essa",
//!   "até"… antes, "-feira" ou "que vem" depois, ou no começo ou no fim do
//!   texto: "revisar a segunda versão" não é segunda-feira;
//! - data sem ano que já passou é a do ano que vem; "dia 5" depois do dia 5 é o
//!   do mês que vem.

use std::sync::LazyLock;

use chrono::{Datelike, Duration, Months, NaiveDate, NaiveTime, Weekday};
use regex::{Captures, Regex};

/// O que saiu de um texto de tarefa.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct ParsedTask {
    /// O texto sem as datas e horas (com a primeira letra maiúscula).
    pub title: String,
    /// Dia planejado.
    pub planned_on: Option<NaiveDate>,
    /// Hora, `HH:MM`.
    pub planned_time: Option<String>,
    /// Prazo ("até sexta").
    pub due_on: Option<NaiveDate>,
}

impl ParsedTask {
    /// Achou alguma data ou hora no texto.
    pub fn has_when(&self) -> bool {
        self.planned_on.is_some() || self.planned_time.is_some() || self.due_on.is_some()
    }
}

/// Um trecho de quando, já resolvido.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct When {
    /// O dia.
    pub date: Option<NaiveDate>,
    /// A hora.
    pub time: Option<NaiveTime>,
    /// Era um prazo ("até …").
    pub due: bool,
}

/// Separa as datas e horas do texto de uma tarefa, em relação a `today`.
///
/// Se tirar as datas deixaria o título vazio ("amanhã"), nada é tirado: o
/// texto inteiro fica como título e sem data.
pub fn parse_task(text: &str, today: NaiveDate) -> ParsedTask {
    let original = text.trim();
    let found = scan(original, today);
    let mut title = String::new();
    let mut last = 0;
    for m in &found {
        title.push_str(&original[last..m.start]);
        title.push(' ');
        last = m.end;
    }
    title.push_str(&original[last..]);
    let title = tidy_title(&title);
    if title.is_empty() {
        return ParsedTask {
            title: capitalize(original),
            ..ParsedTask::default()
        };
    }
    let (planned_on, planned_time, due_on) = combine(&found, today);
    ParsedTask {
        title,
        planned_on,
        planned_time: planned_time.map(|t| t.format("%H:%M").to_string()),
        due_on,
    }
}

/// Resolve um trecho que é só quando ("sexta que vem às 10", "até dia 15").
/// Trecho sem nada reconhecível dá um [`When`] vazio.
pub fn resolve(phrase: &str, today: NaiveDate) -> When {
    let found = scan(phrase.trim(), today);
    let (date, time, due) = combine(&found, today);
    match (date, due) {
        (None, Some(d)) => When {
            date: Some(d),
            time,
            due: true,
        },
        (date, _) => When {
            date,
            time,
            due: false,
        },
    }
}

/// Junta os achados: o primeiro dia vira o planejado, o primeiro prazo vira o
/// prazo, a primeira hora vira a hora. Hora sem dia é hoje.
fn combine(
    found: &[Found],
    today: NaiveDate,
) -> (Option<NaiveDate>, Option<NaiveTime>, Option<NaiveDate>) {
    let planned = found.iter().find_map(|f| match f.what {
        What::Date(d) if !f.due => Some(d),
        _ => None,
    });
    let due = found.iter().find_map(|f| match f.what {
        What::Date(d) if f.due => Some(d),
        _ => None,
    });
    let time = found.iter().find_map(|f| match f.what {
        What::Time(t) => Some(t),
        What::Date(_) => None,
    });
    let planned = planned.or_else(|| time.map(|_| today));
    (planned, time, due)
}

// ------------------------------------------------------------------ busca

#[derive(Debug, Clone, Copy)]
enum What {
    Date(NaiveDate),
    Time(NaiveTime),
}

#[derive(Debug, Clone, Copy)]
struct Found {
    /// Início e fim no texto original (bytes).
    start: usize,
    end: usize,
    what: What,
    due: bool,
}

/// Texto em minúsculas e sem acento, com o caminho de volta para o original:
/// `back[i]` é o byte do original de onde veio o byte `i` do dobrado.
struct Folded {
    text: String,
    back: Vec<usize>,
}

fn fold(original: &str) -> Folded {
    let mut text = String::with_capacity(original.len());
    let mut back = Vec::with_capacity(original.len() + 1);
    for (at, c) in original.char_indices() {
        let base = match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => 'a',
            'é' | 'ê' | 'è' | 'É' | 'Ê' | 'È' => 'e',
            'í' | 'î' | 'Í' | 'Î' => 'i',
            'ó' | 'ô' | 'õ' | 'ò' | 'Ó' | 'Ô' | 'Õ' | 'Ò' => 'o',
            'ú' | 'ü' | 'Ú' | 'Ü' => 'u',
            'ç' | 'Ç' => 'c',
            other => other,
        };
        for lower in base.to_lowercase() {
            let before = text.len();
            text.push(lower);
            back.extend(std::iter::repeat_n(at, text.len() - before));
        }
    }
    back.push(original.len());
    Folded { text, back }
}

/// Prazo: "até", "prazo", "prazo até", "entregar até".
const DUE: &str = r"(?:(?P<due>ate|prazo(?:\s+(?:ate|para|de))?|entregar?\s+ate)\s+)?";

fn re(pattern: &str) -> Regex {
    // Os padrões são constantes deste arquivo; um erro neles é bug, e o teste
    // de qualquer frase o pega na hora.
    #[allow(clippy::expect_used)]
    Regex::new(pattern).expect("padrão de data válido")
}

static REL: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        r"\b{DUE}(?:(?:para|pra)\s+)?(?P<rel>depois\s+de\s+amanha|amanha|hoje|hj)(?:\s+(?:de|a|pela|na)\s+(?:manha|tarde|noite))?\b"
    ))
});
static WEEKDAY: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        r"\b{DUE}(?P<det>(?:(?:para|pra)\s+)?(?:na\s+proxima|no\s+proximo|nessa|nesse|nesta|neste|essa|esse|esta|este|proxima|proximo|na|no)\s+)?(?P<wd>segunda|terca|quarta|quinta|sexta|sabado|domingo)(?P<feira>-feira|\s+feira)?(?P<next>\s+que\s+vem)?(?:\s+(?:de|a|pela)\s+(?:manha|tarde|noite))?\b"
    ))
});
static NEXT_WEEK: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        r"\b{DUE}(?:(?:na|para\s+a|pra)\s+)?(?:semana\s+que\s+vem|proxima\s+semana)\b"
    ))
});
static WEEKEND: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        r"\b{DUE}(?:(?:no|neste|nesse|para\s+o|pro)\s+)?(?:fim|final)\s+de\s+semana\b"
    ))
});
static MONTH_END: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        r"\b{DUE}(?:(?:no|ate\s+o|para\s+o|pro)\s+)?(?:fim|final)\s+do\s+mes\b"
    ))
});
static SLASH_DATE: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        r"\b{DUE}(?:(?:no\s+dia|para\s+o\s+dia|pro\s+dia|dia|em|no)\s+)?(?P<d>\d{{1,2}})/(?P<m>\d{{1,2}})(?:/(?P<y>\d{{2,4}}))?\b"
    ))
});
static NAMED_DATE: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        r"\b{DUE}(?:(?:no\s+dia|para\s+o\s+dia|pro\s+dia|dia|em|no)\s+)?(?P<d>\d{{1,2}})\s+de\s+(?P<mon>janeiro|fevereiro|marco|abril|maio|junho|julho|agosto|setembro|outubro|novembro|dezembro)\b"
    ))
});
static DAY_OF_MONTH: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        r"\b{DUE}(?:(?:no|para\s+o|pro|ate\s+o)\s+)?dia\s+(?P<d>\d{{1,2}})\b"
    ))
});
static IN_N: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"\b(?:daqui\s+a|daqui|em)\s+(?P<n>\d{1,3}|um|uma|dois|duas|tres)\s+(?P<u>dias?|semanas?|mes|meses)\b",
    )
});
static CLOCK: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"\b(?:(?:as|a|ao|pelas|por\s+volta\s+das)\s+)?(?P<h>\d{1,2})(?::|h)(?P<mi>\d{2})\b(?:\s+da\s+(?P<per>manha|tarde|noite))?",
    )
});
static HOURS: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"\b(?:(?:as|a|pelas|por\s+volta\s+das)\s+)?(?P<h>\d{1,2})\s*(?:hs?|hrs?|horas?)\b(?:\s+da\s+(?P<per>manha|tarde|noite))?",
    )
});
static SPOKEN: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"\b(?:(?P<art>as|a)|pelas|por\s+volta\s+das)\s+(?P<h>\d{1,2}|uma|duas|tres|quatro|cinco|seis|sete|oito|nove|dez|onze|doze)(?P<half>\s+e\s+meia)?(?:\s+da\s+(?P<per>manha|tarde|noite))?\b",
    )
});
static MIDDAY: LazyLock<Regex> = LazyLock::new(|| {
    re(r"\b(?:(?:ao|as|a|para\s+o|pro)\s+)?(?P<mn>meio[\s-]dia|meia[\s-]noite)\b")
});

/// Achados no texto dobrado, antes de voltar para o original.
#[derive(Default)]
struct Spans {
    found: Vec<(usize, usize, What, bool)>,
    /// Trechos que pareciam data mas não existem ("31 de fevereiro"): não
    /// viram nada e também não deixam um padrão menor ("dia 31") pegá-los.
    blocked: Vec<(usize, usize)>,
}

impl Spans {
    fn free(&self, (start, end): (usize, usize)) -> bool {
        !self.found.iter().any(|f| start < f.1 && f.0 < end)
            && !self.blocked.iter().any(|b| start < b.1 && b.0 < end)
    }

    /// Guarda o achado, ou bloqueia o trecho quando a data não existe.
    fn put(&mut self, span: (usize, usize), what: Option<What>, due: bool) {
        if !self.free(span) {
            return;
        }
        match what {
            Some(w) => self.found.push((span.0, span.1, w, due)),
            None => self.blocked.push(span),
        }
    }
}

/// Todas as datas e horas do texto, sem sobreposição, na ordem do texto.
fn scan(original: &str, today: NaiveDate) -> Vec<Found> {
    let f = fold(original);
    let text = f.text.as_str();
    let mut s = Spans::default();
    let due = |c: &Captures<'_>| c.name("due").is_some();
    let date = |d: Option<NaiveDate>| d.map(What::Date);
    // Mais específico primeiro: data com barra antes de "dia N", "depois de
    // amanhã" antes de "amanhã", hora com minutos antes de hora cheia.
    for c in SLASH_DATE.captures_iter(text) {
        s.put(whole(&c), date(slash_date(&c, today)), due(&c));
    }
    for c in NAMED_DATE.captures_iter(text) {
        s.put(whole(&c), date(named_date(&c, today)), due(&c));
    }
    for c in REL.captures_iter(text) {
        let days = match c.name("rel").map(|m| m.as_str()) {
            Some("hoje" | "hj") => 0,
            Some("amanha") => 1,
            _ => 2,
        };
        s.put(
            whole(&c),
            Some(What::Date(today + Duration::days(days))),
            due(&c),
        );
    }
    for c in NEXT_WEEK.captures_iter(text) {
        let monday = today + Duration::days(7 - i64::from(today.weekday().num_days_from_monday()));
        s.put(whole(&c), Some(What::Date(monday)), due(&c));
    }
    for c in WEEKEND.captures_iter(text) {
        let d = match today.weekday() {
            Weekday::Sat | Weekday::Sun => today,
            w => today + Duration::days(5 - i64::from(w.num_days_from_monday())),
        };
        s.put(whole(&c), Some(What::Date(d)), due(&c));
    }
    for c in MONTH_END.captures_iter(text) {
        s.put(
            whole(&c),
            date(last_day_of_month(today.year(), today.month())),
            due(&c),
        );
    }
    for c in WEEKDAY.captures_iter(text) {
        let m = whole(&c);
        if weekday_counts(&c, text, m) {
            s.put(m, date(weekday_date(&c, today)), due(&c));
        }
    }
    for c in DAY_OF_MONTH.captures_iter(text) {
        s.put(whole(&c), date(day_of_month(&c, today)), due(&c));
    }
    for c in IN_N.captures_iter(text) {
        s.put(whole(&c), date(in_n(&c, today)), false);
    }
    for c in CLOCK.captures_iter(text) {
        s.put(whole(&c), clock_time(&c).map(What::Time), false);
    }
    for c in HOURS.captures_iter(text) {
        s.put(whole(&c), hours_time(&c).map(What::Time), false);
    }
    for c in SPOKEN.captures_iter(text) {
        // "às 3" é hora; "as 3 propostas" é artigo. Sem "h" nem ":", só vale
        // com crase (o Whisper escreve "às"), "pelas" ou "por volta das".
        let m = whole(&c);
        let lead = &original[f.back[m.0]..f.back[m.1]];
        if c.name("art").is_none() || lead.starts_with('à') || lead.starts_with('À') {
            s.put(m, spoken_time(&c).map(What::Time), false);
        }
    }
    for c in MIDDAY.captures_iter(text) {
        let noon = c.name("mn").is_some_and(|m| m.as_str().starts_with("meio"));
        let t = if noon {
            NaiveTime::from_hms_opt(12, 0, 0)
        } else {
            NaiveTime::from_hms_opt(0, 0, 0)
        };
        s.put(whole(&c), t.map(What::Time), false);
    }
    let mut out: Vec<Found> = s
        .found
        .into_iter()
        .map(|(start, end, what, due)| Found {
            start: f.back[start],
            end: f.back[end],
            what,
            due,
        })
        .collect();
    out.sort_by_key(|f| f.start);
    out
}

fn whole(c: &Captures<'_>) -> (usize, usize) {
    c.get(0).map_or((0, 0), |m| (m.start(), m.end()))
}

/// Dia da semana solto só vale com contexto (ver o topo do arquivo).
fn weekday_counts(c: &Captures<'_>, text: &str, (start, end): (usize, usize)) -> bool {
    let context = c.name("det").is_some()
        || c.name("due").is_some()
        || c.name("feira").is_some()
        || c.name("next").is_some();
    let edge = |s: &str| s.chars().all(|ch| !ch.is_alphanumeric());
    context || edge(&text[..start]) || edge(&text[end..])
}

fn weekday_date(c: &Captures<'_>, today: NaiveDate) -> Option<NaiveDate> {
    let wd = match c.name("wd")?.as_str() {
        "segunda" => Weekday::Mon,
        "terca" => Weekday::Tue,
        "quarta" => Weekday::Wed,
        "quinta" => Weekday::Thu,
        "sexta" => Weekday::Fri,
        "sabado" => Weekday::Sat,
        _ => Weekday::Sun,
    };
    let ahead = (7 + i64::from(wd.num_days_from_monday())
        - i64::from(today.weekday().num_days_from_monday()))
        % 7;
    let mut d = today + Duration::days(if ahead == 0 { 7 } else { ahead });
    let det = c.name("det").map_or("", |m| m.as_str());
    let next = c.name("next").is_some() || det.contains("proxim");
    if next && d.iso_week() == today.iso_week() {
        d += Duration::days(7);
    }
    Some(d)
}

fn month_number(name: &str) -> Option<u32> {
    Some(match name {
        "janeiro" => 1,
        "fevereiro" => 2,
        "marco" => 3,
        "abril" => 4,
        "maio" => 5,
        "junho" => 6,
        "julho" => 7,
        "agosto" => 8,
        "setembro" => 9,
        "outubro" => 10,
        "novembro" => 11,
        "dezembro" => 12,
        _ => return None,
    })
}

/// Dia e mês sem ano: este ano, ou o que vem se já passou.
fn next_occurrence(day: u32, month: u32, today: NaiveDate) -> Option<NaiveDate> {
    let this_year = NaiveDate::from_ymd_opt(today.year(), month, day)?;
    if this_year >= today {
        Some(this_year)
    } else {
        NaiveDate::from_ymd_opt(today.year() + 1, month, day)
    }
}

fn number(c: &Captures<'_>, name: &str) -> Option<u32> {
    c.name(name)?.as_str().parse().ok()
}

fn slash_date(c: &Captures<'_>, today: NaiveDate) -> Option<NaiveDate> {
    let (d, m) = (number(c, "d")?, number(c, "m")?);
    match c.name("y").and_then(|y| y.as_str().parse::<i32>().ok()) {
        Some(y) => NaiveDate::from_ymd_opt(if y < 100 { 2000 + y } else { y }, m, d),
        None => next_occurrence(d, m, today),
    }
}

fn named_date(c: &Captures<'_>, today: NaiveDate) -> Option<NaiveDate> {
    next_occurrence(
        number(c, "d")?,
        month_number(c.name("mon")?.as_str())?,
        today,
    )
}

/// "dia 12": deste mês, ou do que vem se já passou (e se o mês tiver o dia).
fn day_of_month(c: &Captures<'_>, today: NaiveDate) -> Option<NaiveDate> {
    let d = number(c, "d")?;
    if let Some(date) = NaiveDate::from_ymd_opt(today.year(), today.month(), d)
        && date >= today
    {
        return Some(date);
    }
    let next = today.checked_add_months(Months::new(1))?;
    NaiveDate::from_ymd_opt(next.year(), next.month(), d)
}

fn last_day_of_month(year: i32, month: u32) -> Option<NaiveDate> {
    let first_next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)?
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)?
    };
    first_next.pred_opt()
}

fn word_number(s: &str) -> Option<u32> {
    Some(match s {
        "um" | "uma" => 1,
        "dois" | "duas" => 2,
        "tres" => 3,
        "quatro" => 4,
        "cinco" => 5,
        "seis" => 6,
        "sete" => 7,
        "oito" => 8,
        "nove" => 9,
        "dez" => 10,
        "onze" => 11,
        "doze" => 12,
        n => return n.parse().ok(),
    })
}

fn in_n(c: &Captures<'_>, today: NaiveDate) -> Option<NaiveDate> {
    let n = word_number(c.name("n")?.as_str())?;
    let unit = c.name("u")?.as_str();
    if unit.starts_with("dia") {
        today.checked_add_signed(Duration::days(i64::from(n)))
    } else if unit.starts_with("semana") {
        today.checked_add_signed(Duration::weeks(i64::from(n)))
    } else {
        today.checked_add_months(Months::new(n))
    }
}

/// Hora falada: "da tarde/noite" soma 12; sem período, de 1 a 6 é da tarde.
fn spoken_hour(h: u32, period: Option<&str>, ambiguous: bool) -> Option<u32> {
    let h = match period {
        Some("manha") => h,
        Some(_) if h < 12 => h + 12,
        Some(_) => h,
        None if ambiguous && (1..=6).contains(&h) => h + 12,
        None => h,
    };
    (h < 24).then_some(h)
}

fn clock_time(c: &Captures<'_>) -> Option<NaiveTime> {
    let raw = c.name("h")?.as_str();
    let h = raw.parse().ok()?;
    let mi = number(c, "mi")?;
    // "3:30" é da tarde; "03:30", com o zero, é de madrugada.
    let h = spoken_hour(h, c.name("per").map(|m| m.as_str()), raw.len() == 1)?;
    NaiveTime::from_hms_opt(h, mi, 0)
}

fn hours_time(c: &Captures<'_>) -> Option<NaiveTime> {
    let raw = c.name("h")?.as_str();
    let h = spoken_hour(
        raw.parse().ok()?,
        c.name("per").map(|m| m.as_str()),
        raw.len() == 1,
    )?;
    NaiveTime::from_hms_opt(h, 0, 0)
}

fn spoken_time(c: &Captures<'_>) -> Option<NaiveTime> {
    let h = word_number(c.name("h")?.as_str())?;
    let h = spoken_hour(h, c.name("per").map(|m| m.as_str()), true)?;
    let mi = if c.name("half").is_some() { 30 } else { 0 };
    NaiveTime::from_hms_opt(h, mi, 0)
}

// --------------------------------------------------------------- título

/// Palavras que sobram penduradas nas pontas depois de tirar a data.
const DANGLING: &[&str] = &[
    "para", "pra", "pro", "de", "do", "da", "em", "no", "na", "e", "as", "às", "a", "o", "ate",
    "até",
];

fn tidy_title(raw: &str) -> String {
    let mut words: Vec<&str> = raw.split_whitespace().collect();
    let junk = |w: &str| {
        let bare = w.trim_matches(|c: char| !c.is_alphanumeric());
        bare.is_empty() || DANGLING.contains(&bare.to_lowercase().as_str())
    };
    while words.first().is_some_and(|w| junk(w)) {
        words.remove(0);
    }
    while words.last().is_some_and(|w| junk(w)) {
        words.pop();
    }
    let joined = words.join(" ");
    let trimmed = joined
        .trim_matches(|c: char| matches!(c, ',' | ';' | ':' | '-' | '–' | '—'))
        .trim();
    capitalize(trimmed)
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Quarta-feira, 7 de outubro de 2026.
    fn quarta() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 7).unwrap()
    }

    fn d(s: &str) -> Option<NaiveDate> {
        Some(NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap())
    }

    fn p(text: &str) -> ParsedTask {
        parse_task(text, quarta())
    }

    fn t(s: &str) -> Option<String> {
        Some(s.to_string())
    }

    #[test]
    fn amanha_as_3_e_da_tarde() {
        let r = p("amanhã às 3 ligar pro João");
        assert_eq!(r.title, "Ligar pro João");
        assert_eq!(r.planned_on, d("2026-10-08"));
        assert_eq!(r.planned_time, t("15:00"));
        let r = p("Ligar pro João amanhã às 15h");
        assert_eq!(r.title, "Ligar pro João");
        assert_eq!(r.planned_time, t("15:00"));
    }

    #[test]
    fn caixa_e_acento_nao_importam() {
        let r = p("AMANHA revisar o contrato");
        assert_eq!(r.planned_on, d("2026-10-08"));
        assert_eq!(r.title, "Revisar o contrato");
    }

    #[test]
    fn dias_da_semana() {
        assert_eq!(p("revisar o contrato sexta").planned_on, d("2026-10-09"));
        assert_eq!(p("na sexta-feira revisar").planned_on, d("2026-10-09"));
        assert_eq!(
            p("revisar o contrato na sexta que vem").planned_on,
            d("2026-10-16"),
            "a sexta desta semana pula para a próxima"
        );
        assert_eq!(p("próxima sexta deploy").planned_on, d("2026-10-16"));
        assert_eq!(
            p("segunda que vem planejar").planned_on,
            d("2026-10-12"),
            "a próxima segunda já é da semana que vem"
        );
        assert_eq!(
            p("quarta revisar").planned_on,
            d("2026-10-14"),
            "hoje é quarta: a próxima"
        );
    }

    #[test]
    fn dia_da_semana_solto_no_meio_nao_vale() {
        let r = p("Revisar a segunda versão do documento");
        assert!(!r.has_when());
        assert_eq!(r.title, "Revisar a segunda versão do documento");
        assert_eq!(p("segunda revisar o deploy").planned_on, d("2026-10-12"));
    }

    #[test]
    fn prazo_vai_para_o_prazo() {
        let r = p("pagar o boleto até sexta");
        assert_eq!(r.title, "Pagar o boleto");
        assert_eq!(r.due_on, d("2026-10-09"));
        assert_eq!(r.planned_on, None);
        let r = p("amanhã revisar o relatório, prazo dia 15");
        assert_eq!(r.planned_on, d("2026-10-08"));
        assert_eq!(r.due_on, d("2026-10-15"));
        assert_eq!(r.title, "Revisar o relatório");
    }

    #[test]
    fn datas_com_numero() {
        assert_eq!(p("dia 12 entregar relatório").planned_on, d("2026-10-12"));
        assert_eq!(p("dia 5 pagar aluguel").planned_on, d("2026-11-05"));
        assert_eq!(p("12/10 apresentação").planned_on, d("2026-10-12"));
        assert_eq!(p("06/10 renovar").planned_on, d("2027-10-06"));
        assert_eq!(p("01/02/2027 renovar").planned_on, d("2027-02-01"));
        assert_eq!(p("15 de novembro feriado").planned_on, d("2026-11-15"));
        assert_eq!(p("dia 31 de fevereiro").planned_on, None, "não existe");
    }

    #[test]
    fn relativos() {
        assert_eq!(p("depois de amanhã ligar").planned_on, d("2026-10-09"));
        assert_eq!(p("daqui a 3 dias ligar").planned_on, d("2026-10-10"));
        assert_eq!(p("em duas semanas revisar").planned_on, d("2026-10-21"));
        assert_eq!(
            p("semana que vem planejar sprint").planned_on,
            d("2026-10-12")
        );
        assert_eq!(
            p("no fim de semana lavar o carro").planned_on,
            d("2026-10-10")
        );
        assert_eq!(p("fim do mês fechar notas").planned_on, d("2026-10-31"));
    }

    #[test]
    fn horas() {
        let r = p("reunião às 9 e meia");
        assert_eq!(r.planned_on, d("2026-10-07"), "hora sem dia é hoje");
        assert_eq!(r.planned_time, t("09:30"));
        assert_eq!(p("às 15:30 call com o cliente").planned_time, t("15:30"));
        assert_eq!(p("às 7 da noite jantar").planned_time, t("19:00"));
        assert_eq!(p("às 5 da manhã correr").planned_time, t("05:00"));
        assert_eq!(p("meio-dia almoço com o time").planned_time, t("12:00"));
        assert_eq!(p("03:30 backup").planned_time, t("03:30"));
        assert_eq!(p("3:30 revisar").planned_time, t("15:30"));
        assert_eq!(p("às 25h nada").planned_time, None);
    }

    #[test]
    fn artigo_as_nao_vira_hora() {
        let r = p("revisar as 3 propostas");
        assert!(!r.has_when());
        assert_eq!(r.title, "Revisar as 3 propostas");
        assert_eq!(p("pelas 4 ligar").planned_time, t("16:00"));
        assert_eq!(
            p("as 15h ligar").planned_time,
            t("15:00"),
            "com h vale sem crase"
        );
    }

    #[test]
    fn periodo_do_dia_sai_do_titulo_sem_virar_hora() {
        let r = p("amanhã de manhã revisar o PR");
        assert_eq!(r.planned_on, d("2026-10-08"));
        assert_eq!(r.planned_time, None);
        assert_eq!(r.title, "Revisar o PR");
    }

    #[test]
    fn so_a_data_fica_como_titulo() {
        let r = p("amanhã");
        assert_eq!(r.title, "Amanhã");
        assert!(!r.has_when());
    }

    #[test]
    fn texto_sem_data_so_ganha_maiuscula() {
        let r = p("ligar 3x pro cliente");
        assert!(!r.has_when());
        assert_eq!(r.title, "Ligar 3x pro cliente");
    }

    #[test]
    fn resolve_um_trecho() {
        let w = resolve("sexta que vem às 10", quarta());
        assert_eq!(w.date, d("2026-10-16"));
        assert_eq!(w.time, NaiveTime::from_hms_opt(10, 0, 0));
        assert!(!w.due);
        let w = resolve("até dia 15", quarta());
        assert_eq!(w.date, d("2026-10-15"));
        assert!(w.due);
        assert_eq!(resolve("qualquer coisa", quarta()), When::default());
    }

    #[test]
    fn hoje_sendo_sexta_a_sexta_e_a_da_semana_que_vem() {
        let sexta = NaiveDate::from_ymd_opt(2026, 10, 9).unwrap();
        assert_eq!(
            parse_task("sexta revisar", sexta).planned_on,
            d("2026-10-16")
        );
        assert_eq!(
            parse_task("no fim de semana", sexta).planned_on,
            None,
            "só a data: vira título"
        );
        assert_eq!(
            parse_task("no fim de semana lavar o carro", sexta).planned_on,
            d("2026-10-10")
        );
    }

    proptest::proptest! {
        /// Qualquer texto: nada de pânico (fatias fora de fronteira de
        /// caractere, acentos, emoji) e o título nunca fica vazio se o texto
        /// não era vazio.
        #[test]
        fn qualquer_texto_sai_inteiro(s in "\\PC{0,60}") {
            let r = parse_task(&s, quarta());
            if !s.trim().is_empty() {
                proptest::prop_assert!(!r.title.is_empty());
            }
        }

        /// Frases com data no meio de texto qualquer continuam sem pânico.
        #[test]
        fn data_no_meio_de_qualquer_coisa(a in "\\PC{0,20}", b in "\\PC{0,20}") {
            let _ = parse_task(&format!("{a} amanhã às 3 {b}"), quarta());
            let _ = parse_task(&format!("{a} até sexta que vem às 9 e meia {b}"), quarta());
        }
    }
}
