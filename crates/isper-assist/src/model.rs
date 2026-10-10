//! O que o assistente guarda: tarefas, o pedido de mudança e o diário.
//!
//! Os nomes que vão para o banco e para a interface são os do `serde`
//! (`snake_case`): `open`, `done`, `notion`… A interface traduz.

use chrono::NaiveDate;
use serde::{Deserialize, Deserializer, Serialize};

use crate::recur::Rule;
use crate::{AssistError, Result};

/// Título mais longo aceito (caracteres).
pub const MAX_TITLE_CHARS: usize = 500;
/// Notas mais longas aceitas (caracteres).
pub const MAX_NOTES_CHARS: usize = 20_000;
/// Prioridade mais alta (0 é "sem prioridade").
pub const MAX_PRIORITY: u8 = 3;

/// Em que pé está a tarefa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Chegou de fora (reunião, importação, DevOps) e espera triagem.
    Inbox,
    /// Aceita, por fazer.
    Open,
    /// Feita.
    Done,
    /// Descartada. Não some: dá para reabrir.
    Dropped,
}

impl TaskStatus {
    /// O nome no banco.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Open => "open",
            Self::Done => "done",
            Self::Dropped => "dropped",
        }
    }

    /// Lê o nome do banco.
    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "inbox" => Self::Inbox,
            "open" => Self::Open,
            "done" => Self::Done,
            "dropped" => Self::Dropped,
            other => {
                return Err(AssistError::Invalid(format!(
                    "estado desconhecido: {other}"
                )));
            }
        })
    }
}

/// De onde a tarefa veio. Toda tarefa sabe a origem (ADR 0021).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// Digitada na tela Hoje ou na paleta.
    Manual,
    /// Ditada (10.1).
    Voice,
    /// Dita numa reunião gravada (10.3).
    Meeting,
    /// Confirmada como ação no Copilot.
    Copilot,
    /// Importada do Notion.
    Notion,
    /// Work item do Azure DevOps.
    Devops,
    /// Criada por uma rotina (10.2).
    Routine,
}

impl SourceKind {
    /// O nome no banco.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Voice => "voice",
            Self::Meeting => "meeting",
            Self::Copilot => "copilot",
            Self::Notion => "notion",
            Self::Devops => "devops",
            Self::Routine => "routine",
        }
    }

    /// Lê o nome do banco.
    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "manual" => Self::Manual,
            "voice" => Self::Voice,
            "meeting" => Self::Meeting,
            "copilot" => Self::Copilot,
            "notion" => Self::Notion,
            "devops" => Self::Devops,
            "routine" => Self::Routine,
            other => {
                return Err(AssistError::Invalid(format!(
                    "origem desconhecida: {other}"
                )));
            }
        })
    }
}

/// Quem fez a mudança. Vai para o diário.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    /// A pessoa, pela interface.
    User,
    /// O assistente: o que ele acha (as ações de uma reunião) vai para a
    /// caixa de entrada; o que muda fora do ISPer, só depois de um toque.
    Assistant,
    /// Uma rotina (10.2).
    Routine,
    /// Uma importação (Notion).
    Import,
    /// O celular, pela sincronia (10.6): a mudança foi feita lá e chegou depois.
    Phone,
}

impl Actor {
    /// O nome no banco.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Routine => "routine",
            Self::Import => "import",
            Self::Phone => "phone",
        }
    }
}

/// Uma tarefa.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    /// UUID v7.
    pub id: String,
    /// O que fazer.
    pub title: String,
    /// Detalhes (pode ser vazio).
    pub notes: String,
    /// Em que pé está.
    pub status: TaskStatus,
    /// Dia em que a pessoa pretende fazer. Sem dia é "algum dia".
    pub planned_on: Option<NaiveDate>,
    /// Hora marcada naquele dia, `HH:MM`.
    pub planned_time: Option<String>,
    /// Prazo.
    pub due_on: Option<NaiveDate>,
    /// 0 (nenhuma) a 3 (alta).
    pub priority: u8,
    /// Área ou projeto, texto livre.
    pub area: Option<String>,
    /// De onde veio.
    pub source_kind: SourceKind,
    /// Detalhe da origem (o minuto da reunião, a página do Notion…), em JSON.
    pub source_ref: Option<serde_json::Value>,
    /// Id no sistema de origem (`notion:<id>`), único quando existe.
    pub external_ref: Option<String>,
    /// Rotina que criou a tarefa.
    pub routine_id: Option<String>,
    /// Ordem manual; nasce com o instante de criação.
    pub position: f64,
    /// Criada em (ms UTC).
    pub created_at: i64,
    /// Última mudança (ms UTC).
    pub updated_at: i64,
    /// Concluída em (ms UTC).
    pub completed_at: Option<i64>,
}

/// Uma tarefa nova.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewTask {
    /// O que fazer.
    pub title: String,
    /// Detalhes.
    #[serde(default)]
    pub notes: String,
    /// `open` (padrão) ou `inbox`.
    #[serde(default = "default_new_status")]
    pub status: TaskStatus,
    /// Dia planejado.
    #[serde(default)]
    pub planned_on: Option<NaiveDate>,
    /// Hora, `HH:MM`.
    #[serde(default)]
    pub planned_time: Option<String>,
    /// Prazo.
    #[serde(default)]
    pub due_on: Option<NaiveDate>,
    /// 0 a 3.
    #[serde(default)]
    pub priority: u8,
    /// Área ou projeto.
    #[serde(default)]
    pub area: Option<String>,
    /// De onde veio.
    #[serde(default = "default_source")]
    pub source_kind: SourceKind,
    /// Detalhe da origem.
    #[serde(default)]
    pub source_ref: Option<serde_json::Value>,
    /// Id no sistema de origem.
    #[serde(default)]
    pub external_ref: Option<String>,
}

fn default_new_status() -> TaskStatus {
    TaskStatus::Open
}

fn default_source() -> SourceKind {
    SourceKind::Manual
}

impl NewTask {
    /// Uma tarefa digitada, só com o título.
    pub fn titled(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            notes: String::new(),
            status: TaskStatus::Open,
            planned_on: None,
            planned_time: None,
            due_on: None,
            priority: 0,
            area: None,
            source_kind: SourceKind::Manual,
            source_ref: None,
            external_ref: None,
        }
    }

    /// Valida e normaliza (apara o título, a área e a hora).
    pub(crate) fn normalized(mut self) -> Result<Self> {
        self.title = clean_title(&self.title)?;
        check_notes(&self.notes)?;
        if !matches!(self.status, TaskStatus::Open | TaskStatus::Inbox) {
            return Err(AssistError::Invalid(
                "uma tarefa nova nasce aberta ou na caixa de entrada".into(),
            ));
        }
        self.planned_time = clean_time(self.planned_time)?;
        if self.planned_time.is_some() && self.planned_on.is_none() {
            return Err(AssistError::Invalid("hora sem dia".into()));
        }
        check_priority(self.priority)?;
        self.area = clean_optional(self.area);
        self.external_ref = clean_optional(self.external_ref);
        Ok(self)
    }
}

/// Mudança numa tarefa. Campo ausente fica como está; nos campos que podem
/// ficar vazios, `null` limpa.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct TaskPatch {
    /// Novo título.
    #[serde(default)]
    pub title: Option<String>,
    /// Novas notas.
    #[serde(default)]
    pub notes: Option<String>,
    /// Novo dia (`null` = algum dia).
    #[serde(default, deserialize_with = "double_option")]
    pub planned_on: Option<Option<NaiveDate>>,
    /// Nova hora (`null` limpa).
    #[serde(default, deserialize_with = "double_option")]
    pub planned_time: Option<Option<String>>,
    /// Novo prazo (`null` limpa).
    #[serde(default, deserialize_with = "double_option")]
    pub due_on: Option<Option<NaiveDate>>,
    /// Nova prioridade.
    #[serde(default)]
    pub priority: Option<u8>,
    /// Nova área (`null` limpa).
    #[serde(default, deserialize_with = "double_option")]
    pub area: Option<Option<String>>,
}

/// Distingue "campo ausente" (`None`) de "campo `null`" (`Some(None)`).
fn double_option<'de, D, T>(d: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

impl TaskPatch {
    /// Aplica a mudança sobre uma cópia da tarefa, validando.
    pub(crate) fn apply(self, task: &Task) -> Result<Task> {
        let mut out = task.clone();
        if let Some(title) = self.title {
            out.title = clean_title(&title)?;
        }
        if let Some(notes) = self.notes {
            check_notes(&notes)?;
            out.notes = notes;
        }
        if let Some(day) = self.planned_on {
            out.planned_on = day;
            if day.is_none() {
                out.planned_time = None; // sem dia, a hora não faz sentido
            }
        }
        if let Some(time) = self.planned_time {
            out.planned_time = clean_time(time)?;
        }
        if out.planned_time.is_some() && out.planned_on.is_none() {
            return Err(AssistError::Invalid("hora sem dia".into()));
        }
        if let Some(due) = self.due_on {
            out.due_on = due;
        }
        if let Some(priority) = self.priority {
            check_priority(priority)?;
            out.priority = priority;
        }
        if let Some(area) = self.area {
            out.area = clean_optional(area);
        }
        Ok(out)
    }
}

fn clean_title(title: &str) -> Result<String> {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        return Err(AssistError::Invalid("a tarefa precisa de um título".into()));
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(AssistError::Invalid(format!(
            "título com mais de {MAX_TITLE_CHARS} caracteres"
        )));
    }
    Ok(title)
}

fn check_notes(notes: &str) -> Result<()> {
    if notes.chars().count() > MAX_NOTES_CHARS {
        return Err(AssistError::Invalid(format!(
            "notas com mais de {MAX_NOTES_CHARS} caracteres"
        )));
    }
    Ok(())
}

fn check_priority(priority: u8) -> Result<()> {
    if priority > MAX_PRIORITY {
        return Err(AssistError::Invalid(format!(
            "prioridade vai de 0 a {MAX_PRIORITY}"
        )));
    }
    Ok(())
}

/// `H:MM` ou `HH:MM` vira `HH:MM`; vazio vira nada.
fn clean_time(time: Option<String>) -> Result<Option<String>> {
    let Some(time) = clean_optional(time) else {
        return Ok(None);
    };
    let parsed = chrono::NaiveTime::parse_from_str(&time, "%H:%M")
        .map_err(|_| AssistError::Invalid(format!("hora inválida: {time} (use HH:MM)")))?;
    Ok(Some(parsed.format("%H:%M").to_string()))
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// O dia montado para a tela Hoje.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Today {
    /// O dia (local).
    pub day: Option<NaiveDate>,
    /// Abertas para hoje ou atrasadas (dia ou prazo até hoje), na ordem de
    /// fazer: com hora primeiro, depois prioridade, depois a ordem manual.
    pub planned: Vec<Task>,
    /// Esperando triagem.
    pub inbox: Vec<Task>,
    /// Abertas para outro dia ou sem dia.
    pub later: Vec<Task>,
    /// Concluídas hoje, da mais recente para a mais antiga.
    pub done_today: Vec<Task>,
}

/// Verificador que confere no OptTime se o dia fechou a meta de horas
/// (`opt_time_get_today_summary`).
pub const VERIFY_OPTTIME_DAY: &str = "opttime.day_complete";
/// Ação que preenche o dia no OptTime com as sugestões, depois do toque
/// (`opt_time_suggest_daily_entries` e `opt_time_apply_suggestions`).
pub const ACTION_OPTTIME_FILL: &str = "opttime.fill_day";

const VERIFIERS: [&str; 1] = [VERIFY_OPTTIME_DAY];
const ACTIONS: [&str; 1] = [ACTION_OPTTIME_FILL];

/// Quanto a rotina pode fazer sozinha ([ADR 0023]).
///
/// [ADR 0023]: ../../../docs/adr/0023-escada-de-confianca.md
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutineMode {
    /// Quando o verificador confirma, pede um toque para concluir.
    Ask,
    /// Quando o verificador confirma, conclui sem avisar. Só existe em rotina
    /// com verificador; escrever fora do ISPer continua pedindo o toque.
    Auto,
}

impl RoutineMode {
    /// O nome no banco.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Auto => "auto",
        }
    }

    /// Lê o nome do banco.
    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "ask" => Self::Ask,
            "auto" => Self::Auto,
            other => return Err(AssistError::Invalid(format!("modo desconhecido: {other}"))),
        })
    }
}

/// Uma rotina: o que se repete, quando, e como conferir que foi feito.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Routine {
    /// UUID v7.
    pub id: String,
    /// O que fazer; vira o título da tarefa de cada dia.
    pub title: String,
    /// A recorrência, na forma canônica do RRULE.
    pub rrule: String,
    /// A mesma recorrência, aberta para a interface.
    pub rule: Rule,
    /// Como conferir que foi feito ([`VERIFY_OPTTIME_DAY`]).
    pub verifier: Option<String>,
    /// O que fazer quando falta ([`ACTION_OPTTIME_FILL`]).
    pub action: Option<String>,
    /// Quanto pode fazer sozinha.
    pub mode: RoutineMode,
    /// Pausada fica `false`: não cria mais tarefas, mas não some.
    pub active: bool,
    /// A evidência, quando nasceu de uma sugestão (10.5).
    pub learned_from: Option<serde_json::Value>,
    /// Criada em (ms UTC). O dia local conta o `INTERVAL`.
    pub created_at: i64,
    /// Última mudança (ms UTC).
    pub updated_at: i64,
}

/// Uma rotina nova.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NewRoutine {
    /// O que fazer.
    pub title: String,
    /// A recorrência (RRULE).
    pub rrule: String,
    /// Verificador.
    #[serde(default)]
    pub verifier: Option<String>,
    /// Ação.
    #[serde(default)]
    pub action: Option<String>,
    /// `ask` (padrão) ou `auto`.
    #[serde(default = "default_mode")]
    pub mode: RoutineMode,
}

fn default_mode() -> RoutineMode {
    RoutineMode::Ask
}

impl NewRoutine {
    /// "Registrar 8h": dias úteis às 17:00, conferida e preenchida pelo
    /// OptTime ([ADR 0023]).
    ///
    /// [ADR 0023]: ../../../docs/adr/0023-escada-de-confianca.md
    pub fn opttime_hours(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            rrule: Rule::workdays_at(17, 0).to_string(),
            verifier: Some(VERIFY_OPTTIME_DAY.into()),
            action: Some(ACTION_OPTTIME_FILL.into()),
            mode: RoutineMode::Ask,
        }
    }

    /// Valida: título, regra (vai para o banco na forma canônica), verificador
    /// e ação conhecidos, e automático só com verificador.
    pub(crate) fn normalized(self) -> Result<(Self, Rule)> {
        let rule = Rule::parse(&self.rrule)?;
        let out = Self {
            title: clean_title(&self.title)?,
            rrule: rule.to_string(),
            verifier: known(clean_optional(self.verifier), &VERIFIERS, "verificador")?,
            action: known(clean_optional(self.action), &ACTIONS, "ação")?,
            mode: self.mode,
        };
        check_mode(out.mode, out.verifier.as_deref())?;
        Ok((out, rule))
    }
}

/// Mudança numa rotina. Campo ausente fica como está; `null` tira o
/// verificador ou a ação.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct RoutinePatch {
    /// Novo título.
    #[serde(default)]
    pub title: Option<String>,
    /// Nova recorrência.
    #[serde(default)]
    pub rrule: Option<String>,
    /// Novo verificador (`null` tira).
    #[serde(default, deserialize_with = "double_option")]
    pub verifier: Option<Option<String>>,
    /// Nova ação (`null` tira).
    #[serde(default, deserialize_with = "double_option")]
    pub action: Option<Option<String>>,
    /// Novo modo.
    #[serde(default)]
    pub mode: Option<RoutineMode>,
}

impl RoutinePatch {
    /// Aplica sobre uma cópia da rotina, validando.
    pub(crate) fn apply(self, routine: &Routine) -> Result<Routine> {
        let mut out = routine.clone();
        if let Some(title) = self.title {
            out.title = clean_title(&title)?;
        }
        if let Some(rrule) = self.rrule {
            out.rule = Rule::parse(&rrule)?;
            out.rrule = out.rule.to_string();
        }
        if let Some(verifier) = self.verifier {
            out.verifier = known(clean_optional(verifier), &VERIFIERS, "verificador")?;
        }
        if let Some(action) = self.action {
            out.action = known(clean_optional(action), &ACTIONS, "ação")?;
        }
        if let Some(mode) = self.mode {
            out.mode = mode;
        }
        check_mode(out.mode, out.verifier.as_deref())?;
        Ok(out)
    }
}

fn known(value: Option<String>, list: &[&str], what: &str) -> Result<Option<String>> {
    match value {
        Some(v) if !list.contains(&v.as_str()) => {
            Err(AssistError::Invalid(format!("{what} desconhecido: {v}")))
        }
        other => Ok(other),
    }
}

fn check_mode(mode: RoutineMode, verifier: Option<&str>) -> Result<()> {
    if mode == RoutineMode::Auto && verifier.is_none() {
        return Err(AssistError::Invalid(
            "rotina automática precisa de um verificador".into(),
        ));
    }
    Ok(())
}

/// Uma tarefa de rotina ainda aberta, com a rotina que a criou.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Occurrence {
    /// A tarefa do dia.
    pub task: Task,
    /// A rotina.
    pub routine: Routine,
    /// O dia a que a tarefa se refere (o `planned_on` com que nasceu).
    pub day: NaiveDate,
}

/// Uma linha do diário.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JournalEntry {
    /// Id (cresce com o tempo).
    pub id: i64,
    /// Quando (ms UTC).
    pub at: i64,
    /// Dia local.
    pub day: NaiveDate,
    /// Quem fez.
    pub actor: String,
    /// O que aconteceu: `task.created`, `task.completed`, `task.restored`…
    pub action: String,
    /// `task`, por enquanto.
    pub object_kind: String,
    /// Id do objeto.
    pub object_id: String,
    /// O título na hora da mudança, para listar sem buscar a tarefa.
    pub summary: String,
    /// O antes e o depois, em JSON.
    pub data: Option<serde_json::Value>,
}

/// Tamanho máximo de uma memória: um fato curto, não um texto.
pub const MAX_MEMORY_CHARS: usize = 280;

/// O tipo de uma memória.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    /// Um fato sobre a pessoa ("meu gestor é o Carlos").
    Fact,
    /// Um jeito de preferir ("reuniões só depois das 10h").
    Preference,
}

impl MemoryKind {
    /// O nome no banco.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fact => "fact",
            Self::Preference => "preference",
        }
    }

    /// Lê o nome do banco.
    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "fact" => Self::Fact,
            "preference" => Self::Preference,
            other => {
                return Err(AssistError::Invalid(format!(
                    "tipo de memória desconhecido: {other}"
                )));
            }
        })
    }
}

/// Um fato curto sobre a pessoa, que ela vê e edita, e que o assistente lê
/// antes de responder (Fase 10.5, [ADR 0021]).
///
/// [ADR 0021]: ../../../docs/adr/0021-assistente-pessoal-no-isper.md
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    /// UUID v7.
    pub id: String,
    /// O fato, numa linha.
    pub text: String,
    /// Fato ou preferência.
    pub kind: MemoryKind,
    /// Quem escreveu: `user` ou `assistant` (proposta por ele e aceita com
    /// um toque).
    pub origin: String,
    /// De onde veio, quando o assistente propôs (a pergunta), em JSON.
    pub evidence: Option<serde_json::Value>,
    /// Fixada: vai sempre para o assistente, antes das outras.
    pub pinned: bool,
    /// Criada em (ms UTC).
    pub created_at: i64,
    /// Última mudança (ms UTC).
    pub updated_at: i64,
    /// Última vez que o assistente a leu (ms UTC).
    pub last_used_at: Option<i64>,
    /// Arquivada em (ms UTC): sai do assistente, mas não some.
    pub archived_at: Option<i64>,
}

/// Uma memória nova.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NewMemory {
    /// O fato.
    pub text: String,
    /// `fact` (padrão) ou `preference`.
    #[serde(default = "default_memory_kind")]
    pub kind: MemoryKind,
    /// De onde veio.
    #[serde(default)]
    pub evidence: Option<serde_json::Value>,
}

fn default_memory_kind() -> MemoryKind {
    MemoryKind::Fact
}

impl NewMemory {
    /// Um fato digitado.
    pub fn fact(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: MemoryKind::Fact,
            evidence: None,
        }
    }
}

/// Mudança numa memória. Campo ausente fica como está.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct MemoryPatch {
    /// Novo texto.
    #[serde(default)]
    pub text: Option<String>,
    /// Novo tipo.
    #[serde(default)]
    pub kind: Option<MemoryKind>,
    /// Fixar ou soltar.
    #[serde(default)]
    pub pinned: Option<bool>,
}

/// Apara, junta os espaços numa linha só e confere o tamanho.
pub(crate) fn clean_memory(text: &str) -> Result<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return Err(AssistError::Invalid("memória vazia".into()));
    }
    if text.chars().count() > MAX_MEMORY_CHARS {
        return Err(AssistError::Invalid(format!(
            "a memória passa de {MAX_MEMORY_CHARS} caracteres; guarde um fato curto"
        )));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memoria_vira_uma_linha_curta() {
        assert_eq!(
            clean_memory(
                "  Meu gestor
 é o   Carlos "
            )
            .unwrap(),
            "Meu gestor é o Carlos"
        );
        assert!(clean_memory("   ").is_err());
        assert!(clean_memory(&"a".repeat(MAX_MEMORY_CHARS + 1)).is_err());
        assert_eq!(
            MemoryKind::parse("preference").unwrap(),
            MemoryKind::Preference
        );
    }

    fn task() -> Task {
        Task {
            id: "t".into(),
            title: "Revisar o PR".into(),
            notes: String::new(),
            status: TaskStatus::Open,
            planned_on: NaiveDate::from_ymd_opt(2026, 10, 7),
            planned_time: Some("15:00".into()),
            due_on: None,
            priority: 0,
            area: None,
            source_kind: SourceKind::Manual,
            source_ref: None,
            external_ref: None,
            routine_id: None,
            position: 0.0,
            created_at: 0,
            updated_at: 0,
            completed_at: None,
        }
    }

    #[test]
    fn titulo_e_aparado_e_vazio_e_recusado() {
        let t = NewTask::titled("  Ligar   pro João \n")
            .normalized()
            .unwrap();
        assert_eq!(t.title, "Ligar pro João");
        assert!(NewTask::titled("   ").normalized().is_err());
    }

    #[test]
    fn hora_e_normalizada_e_precisa_de_dia() {
        let mut t = NewTask::titled("x");
        t.planned_on = NaiveDate::from_ymd_opt(2026, 10, 8);
        t.planned_time = Some("9:05".into());
        assert_eq!(
            t.clone().normalized().unwrap().planned_time.as_deref(),
            Some("09:05")
        );
        t.planned_time = Some("25:00".into());
        assert!(t.clone().normalized().is_err());
        t.planned_on = None;
        t.planned_time = Some("10:00".into());
        assert!(t.normalized().is_err(), "hora sem dia");
    }

    #[test]
    fn tarefa_nova_nao_nasce_feita() {
        let mut t = NewTask::titled("x");
        t.status = TaskStatus::Done;
        assert!(t.normalized().is_err());
    }

    #[test]
    fn patch_distingue_ausente_de_nulo() {
        let ausente: TaskPatch = serde_json::from_str(r#"{"title":"Novo"}"#).unwrap();
        assert_eq!(ausente.planned_on, None);
        let nulo: TaskPatch = serde_json::from_str(r#"{"planned_on":null}"#).unwrap();
        assert_eq!(nulo.planned_on, Some(None));
        let dia: TaskPatch = serde_json::from_str(r#"{"planned_on":"2026-10-09"}"#).unwrap();
        assert_eq!(dia.planned_on, Some(NaiveDate::from_ymd_opt(2026, 10, 9)));
    }

    #[test]
    fn tirar_o_dia_tira_a_hora() {
        let patch = TaskPatch {
            planned_on: Some(None),
            ..TaskPatch::default()
        };
        let out = patch.apply(&task()).unwrap();
        assert_eq!(out.planned_on, None);
        assert_eq!(out.planned_time, None);
    }

    #[test]
    fn rotina_nova_e_validada_e_canonica() {
        let (r, rule) = NewRoutine {
            rrule: "freq=weekly;byday=fr,mo".into(),
            ..NewRoutine::opttime_hours("  Registrar   8h ")
        }
        .normalized()
        .unwrap();
        assert_eq!(r.title, "Registrar 8h");
        assert_eq!(r.rrule, "FREQ=WEEKLY;BYDAY=MO,FR");
        assert_eq!(rule.by_day.len(), 2);

        let sem_verificador = NewRoutine {
            title: "Tomar água".into(),
            rrule: "FREQ=DAILY".into(),
            verifier: None,
            action: None,
            mode: RoutineMode::Auto,
        };
        assert!(
            sem_verificador.normalized().is_err(),
            "automática só com verificador"
        );
        let estranho = NewRoutine {
            verifier: Some("jira.done".into()),
            ..NewRoutine::opttime_hours("x")
        };
        assert!(estranho.normalized().is_err());
    }

    #[test]
    fn padrao_registrar_8h() {
        let (r, rule) = NewRoutine::opttime_hours("Registrar 8h no OptTime")
            .normalized()
            .unwrap();
        assert_eq!(
            r.rrule,
            "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR;BYHOUR=17;BYMINUTE=0"
        );
        assert_eq!(rule.time_hhmm().as_deref(), Some("17:00"));
        assert_eq!(r.verifier.as_deref(), Some(VERIFY_OPTTIME_DAY));
        assert_eq!(r.action.as_deref(), Some(ACTION_OPTTIME_FILL));
        assert_eq!(r.mode, RoutineMode::Ask);
    }

    #[test]
    fn nomes_do_banco_vao_e_voltam() {
        for s in [
            TaskStatus::Inbox,
            TaskStatus::Open,
            TaskStatus::Done,
            TaskStatus::Dropped,
        ] {
            assert_eq!(TaskStatus::parse(s.as_str()).unwrap(), s);
        }
        for k in [
            SourceKind::Manual,
            SourceKind::Voice,
            SourceKind::Meeting,
            SourceKind::Copilot,
            SourceKind::Notion,
            SourceKind::Devops,
            SourceKind::Routine,
        ] {
            assert_eq!(SourceKind::parse(k.as_str()).unwrap(), k);
        }
        for m in [RoutineMode::Ask, RoutineMode::Auto] {
            assert_eq!(RoutineMode::parse(m.as_str()).unwrap(), m);
        }
    }
}
