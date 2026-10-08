//! O relógio do assistente: que instante é agora e que dia é "hoje".
//!
//! "Hoje" é o dia no fuso local, que é como a pessoa pensa o dia: uma tarefa
//! concluída às 23:30 em São Paulo conta para hoje, mesmo já sendo amanhã em
//! UTC. Os testes usam um [`FixedClock`] com fuso explícito.

use chrono::{FixedOffset, Local, NaiveDate, Offset, TimeZone, Utc};

/// De onde o assistente tira o agora.
pub trait Clock: Send + Sync {
    /// Milissegundos desde a época, em UTC.
    fn now_ms(&self) -> i64;

    /// Diferença do fuso local para UTC num instante, em segundos.
    fn offset_at(&self, at_ms: i64) -> FixedOffset;

    /// O dia local de um instante.
    fn day_of(&self, at_ms: i64) -> NaiveDate {
        let offset = self.offset_at(at_ms);
        Utc.timestamp_millis_opt(at_ms)
            .single()
            .map(|t| t.with_timezone(&offset).date_naive())
            .unwrap_or_default()
    }

    /// O dia local de agora.
    fn today(&self) -> NaiveDate {
        self.day_of(self.now_ms())
    }

    /// Início e fim (exclusivo) de um dia local, em milissegundos UTC.
    fn day_bounds(&self, day: NaiveDate) -> (i64, i64) {
        let start = local_midnight_ms(self, day);
        let end = local_midnight_ms(self, day.succ_opt().unwrap_or(day));
        (start, end)
    }
}

/// Meia-noite local de um dia, em milissegundos UTC. Usa o fuso de meio-dia
/// daquele dia, que é estável mesmo em dia de troca de horário.
fn local_midnight_ms<C: Clock + ?Sized>(clock: &C, day: NaiveDate) -> i64 {
    let naive = day.and_hms_opt(0, 0, 0).unwrap_or_default();
    let noon_guess = naive.and_utc().timestamp_millis() + 12 * 3_600_000;
    let offset = clock.offset_at(noon_guess);
    naive.and_utc().timestamp_millis() - i64::from(offset.local_minus_utc()) * 1000
}

/// O relógio do sistema, com o fuso do Windows.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        Utc::now().timestamp_millis()
    }

    fn offset_at(&self, at_ms: i64) -> FixedOffset {
        Local
            .timestamp_millis_opt(at_ms)
            .single()
            .map(|t| t.offset().fix())
            .unwrap_or_else(|| Local::now().offset().fix())
    }
}

/// Relógio parado, para testes: um instante e um fuso fixos.
#[derive(Debug, Clone, Copy)]
pub struct FixedClock {
    /// O "agora", em milissegundos UTC.
    pub now_ms: i64,
    /// O fuso local.
    pub offset: FixedOffset,
}

impl FixedClock {
    /// Um relógio em `AAAA-MM-DD HH:MM` local, no fuso de `offset_hours`.
    ///
    /// # Panics
    ///
    /// Com data, hora ou fuso inválidos (só em testes).
    pub fn at(local: &str, offset_hours: i32) -> Self {
        let offset = FixedOffset::east_opt(offset_hours * 3600).expect("fuso válido");
        let naive = chrono::NaiveDateTime::parse_from_str(local, "%Y-%m-%d %H:%M")
            .expect("data no formato AAAA-MM-DD HH:MM");
        let now_ms = offset
            .from_local_datetime(&naive)
            .single()
            .expect("instante local sem ambiguidade")
            .timestamp_millis();
        Self { now_ms, offset }
    }

    /// O mesmo relógio, `minutes` minutos depois.
    pub fn plus_minutes(self, minutes: i64) -> Self {
        Self {
            now_ms: self.now_ms + minutes * 60_000,
            ..self
        }
    }
}

impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        self.now_ms
    }

    fn offset_at(&self, _at_ms: i64) -> FixedOffset {
        self.offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hoje_e_o_dia_local_e_nao_o_de_utc() {
        // 23:30 em São Paulo já é dia seguinte em UTC.
        let clock = FixedClock::at("2026-10-07 23:30", -3);
        assert_eq!(clock.today(), NaiveDate::from_ymd_opt(2026, 10, 7).unwrap());
        assert_eq!(
            clock.plus_minutes(31).today(),
            NaiveDate::from_ymd_opt(2026, 10, 8).unwrap()
        );
    }

    #[test]
    fn limites_do_dia_local() {
        let clock = FixedClock::at("2026-10-07 12:00", -3);
        let day = NaiveDate::from_ymd_opt(2026, 10, 7).unwrap();
        let (start, end) = clock.day_bounds(day);
        assert_eq!(end - start, 86_400_000);
        assert_eq!(clock.day_of(start), day);
        assert_eq!(clock.day_of(end - 1), day);
        assert_eq!(clock.day_of(end), day.succ_opt().unwrap());
    }
}
