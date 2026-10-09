//! O agente do ISPer (Fase 10.4, [ADR 0021] e [ADR 0023]): perguntas livres
//! sobre o seu dia, o resumo da manhã e o fechamento do dia.
//!
//! O laço é o do `isper-llm` ([`isper_llm::agent`]); aqui ficam as
//! ferramentas ([`IsperHost`]) e o que se pede ao modelo:
//!
//! - as do ISPer, sobre o `isper.db` (tarefas, diário, reuniões, ditados,
//!   rotinas; criar, mover e concluir tarefa pedem o toque);
//! - as do OptTime, pelo catálogo do MCP, quando o conector está ligado
//!   ([`OptTimeTools`]).
//!
//! [ADR 0021]: ../../../docs/adr/0021-assistente-pessoal-no-isper.md
//! [ADR 0023]: ../../../docs/adr/0023-escada-de-confianca.md

mod local;
mod opttime;

use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Datelike, Local, Weekday};
use isper_llm::ToolSpec;
use isper_llm::agent::{Permission, ToolHost, ToolOutcome};
use local::LocalTools;
use serde_json::Value;

pub use opttime::{OptTimeTools, default_or_chosen};

/// As ferramentas do agente: as do ISPer e, se houver, as do OptTime.
pub struct IsperHost {
    local: LocalTools,
    opttime: Option<OptTimeTools>,
    chosen: HashMap<String, Permission>,
}

impl std::fmt::Debug for IsperHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IsperHost")
            .field("opttime", &self.opttime.is_some())
            .finish_non_exhaustive()
    }
}

impl IsperHost {
    /// As ferramentas sobre o banco `db`, com o OptTime se houver e as
    /// permissões escolhidas nas Configurações (por nome de ferramenta).
    pub fn new(
        db: &Path,
        opttime: Option<OptTimeTools>,
        chosen: HashMap<String, Permission>,
    ) -> Self {
        Self {
            local: LocalTools::new(db),
            opttime,
            chosen,
        }
    }

    /// O conector do OptTime em uso, se houver.
    pub fn opttime(&self) -> Option<&OptTimeTools> {
        self.opttime.as_ref()
    }
}

impl ToolHost for IsperHost {
    fn tools(&self) -> Vec<ToolSpec> {
        let mut tools: Vec<ToolSpec> = LocalTools::specs()
            .into_iter()
            .filter(|t| self.permission(&t.name) != Permission::Never)
            .collect();
        if let Some(ot) = &self.opttime {
            tools.extend(
                ot.specs()
                    .into_iter()
                    .filter(|t| self.permission(&t.name) != Permission::Never),
            );
        }
        tools
    }

    fn permission(&self, name: &str) -> Permission {
        if LocalTools::is_mine(name) {
            return match self.chosen.get(name) {
                // Escrever no ISPer pode ficar liberado; ler não pode virar pergunta.
                Some(p) => *p,
                None => LocalTools::permission(name),
            };
        }
        match &self.opttime {
            Some(ot) => ot.permission(name, &self.chosen),
            None => Permission::Never,
        }
    }

    fn call(&self, name: &str, arguments: &Value) -> ToolOutcome {
        if LocalTools::is_mine(name) {
            return self.local.call(name, arguments);
        }
        match &self.opttime {
            Some(ot) if ot.has(name) => ot.call(name, arguments),
            _ => ToolOutcome::error(format!("ferramenta indisponível: {name}")),
        }
    }

    fn describe(&self, name: &str, arguments: &Value) -> String {
        if LocalTools::is_mine(name) {
            return self.local.describe(name, arguments);
        }
        match &self.opttime {
            Some(ot) => ot.describe(name, arguments),
            None => format!("Rodar {name}"),
        }
    }
}

fn weekday_pt(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "segunda-feira",
        Weekday::Tue => "terça-feira",
        Weekday::Wed => "quarta-feira",
        Weekday::Thu => "quinta-feira",
        Weekday::Fri => "sexta-feira",
        Weekday::Sat => "sábado",
        Weekday::Sun => "domingo",
    }
}

/// As instruções do agente: quem é a pessoa, que horas são e como citar.
pub fn system_prompt(me: Option<&str>, now: DateTime<Local>, with_opttime: bool) -> String {
    let who = me.map(|n| format!(" de {n}")).unwrap_or_default();
    let opttime = if with_opttime {
        "\n- As ferramentas opt_time_* são do OptTime da empresa: agenda do Outlook, horas lançadas, work items do Azure DevOps e sugestões para preencher o dia."
    } else {
        "\n- O OptTime não está conectado: sem agenda do Outlook nem horas. Se a pergunta depender disso, diga."
    };
    format!(
        "Você é o assistente pessoal{who} no ISPer, o app de ditado e reuniões dele. Hoje é {weekday}, {date} ({iso}), {time}.

Como responder:
- Em português, curto e direto, como um colega que sabe da agenda. Tópicos quando houver lista.
- Use as ferramentas para buscar os fatos antes de responder. Não invente tarefa, reunião, horário nem número.
- Se faltar detalhe, chame a ferramenta de novo (por exemplo `reuniao` com `busca`) em vez de pedir para a pessoa abrir ou procurar algo.
- Cite a origem de cada fato com a referência que vem nos resultados, entre colchetes duplos, logo depois do fato: [[tarefa:…]], [[reuniao:42@754]], [[opttime:…]]. Só cite referências que vieram das ferramentas.
- \"Hoje\" é {iso}; \"ontem\" e \"amanhã\" contam a partir daí. Datas nas ferramentas vão como AAAA-MM-DD.
- Para criar, mover ou concluir tarefa e para lançar horas, chame a ferramenta: o ISPer mostra um cartão e a pessoa confirma. Não peça confirmação em texto e não diga que fez antes do resultado.
- Se uma ferramenta falhar ou for recusada, diga o que faltou em uma linha.{opttime}",
        weekday = weekday_pt(now.weekday()),
        date = now.format("%d/%m/%Y"),
        iso = now.format("%Y-%m-%d"),
        time = now.format("%H:%M"),
    )
}

/// O pedido do resumo da manhã.
pub const MORNING: &str = "Monte o resumo da minha manhã: as reuniões de hoje (com o que ficou pendente da última vez, se houver), o que está para hoje e o que está atrasado, e o que chegou na caixa de entrada. Em tópicos curtos, do mais importante para o menos.";

/// O pedido do fechamento do dia.
pub const CLOSING: &str = "Feche o meu dia: o que eu fiz hoje (pelo diário e pelas tarefas concluídas), quantas horas eu tenho lançadas hoje no OptTime e quanto falta, e o que ficou aberto. Sugira o que mover para amanhã e, se eu já tiver dito o que quero, mova com as ferramentas.";

#[cfg(test)]
mod tests {
    use super::*;
    use isper_assist::{Actor, AssistStore, NewTask};
    use isper_llm::agent::{Conversation, Step, advance, resume};
    use isper_llm::chat::{AssistantTurn, ChatReply, StopReason, ToolCall};
    use isper_llm::testing::FakeChat;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn db() -> std::path::PathBuf {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let path =
            std::env::temp_dir().join(format!("isper-agent-test-{}-{n}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        isper_core::store::MeetingStore::open(&path).unwrap();
        path
    }

    fn call(id: &str, name: &str, args: Value) -> ChatReply {
        ChatReply {
            turn: AssistantTurn {
                text: String::new(),
                tool_calls: vec![ToolCall {
                    id: id.into(),
                    name: name.into(),
                    arguments: args,
                }],
                raw: Value::Null,
            },
            stop: StopReason::ToolUse,
        }
    }

    fn says(text: &str) -> ChatReply {
        ChatReply {
            turn: AssistantTurn {
                text: text.into(),
                tool_calls: Vec::new(),
                raw: Value::Null,
            },
            stop: StopReason::EndTurn,
        }
    }

    #[test]
    fn sem_opttime_so_as_do_isper_e_escrever_pede_o_toque() {
        let path = db();
        let host = IsperHost::new(&path, None, HashMap::new());
        let names: Vec<String> = host.tools().into_iter().map(|t| t.name).collect();
        assert!(names.contains(&"tarefas_do_dia".to_string()));
        assert!(!names.iter().any(|n| n.starts_with("opt_time")));
        assert_eq!(host.permission("tarefas_do_dia"), Permission::Allow);
        assert_eq!(host.permission("criar_tarefa"), Permission::Ask);
        assert_eq!(host.permission("opt_time_log_time"), Permission::Never);
        let mut chosen = HashMap::new();
        chosen.insert("criar_tarefa".to_string(), Permission::Never);
        let host = IsperHost::new(&path, None, chosen);
        assert!(
            !host.tools().iter().any(|t| t.name == "criar_tarefa"),
            "bloqueada nem aparece"
        );
    }

    #[test]
    fn pergunta_sobre_o_dia_cita_a_tarefa_e_criar_espera_o_toque() {
        let path = db();
        let store = AssistStore::open(&path).unwrap();
        let today = Local::now().date_naive();
        let t = store
            .create_task(
                NewTask {
                    planned_on: Some(today),
                    ..NewTask::titled("Revisar o contrato da Marca")
                },
                Actor::User,
            )
            .unwrap();
        let host = IsperHost::new(&path, None, HashMap::new());
        let fake = FakeChat::new(vec![
            call("c1", "tarefas_do_dia", json!({"dia": null})),
            says(&format!("Hoje: revisar o contrato [[tarefa:{}]].", t.id)),
            call(
                "c2",
                "criar_tarefa",
                json!({"titulo": "Ligar pro João", "dia": today.to_string(), "hora": "15:00", "prazo": null}),
            ),
            says("Criei a tarefa."),
        ]);
        let system = system_prompt(Some("Marcus"), Local::now(), false);
        let mut conv = Conversation::new("o que eu tenho hoje?");
        let Step::Answer { sources, .. } = advance(&fake, &host, &system, &mut conv).unwrap()
        else {
            panic!("esperava resposta");
        };
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].label, "Revisar o contrato da Marca");
        let sent = fake.messages(1);
        let isper_llm::ChatMessage::Tools { results } = &sent[2] else {
            panic!("esperava o resultado da ferramenta");
        };
        assert!(results[0].content.contains("Revisar o contrato da Marca"));

        conv.ask("cria: ligar pro João hoje às 15h");
        let Step::Confirm { actions } = advance(&fake, &host, &system, &mut conv).unwrap() else {
            panic!("esperava o cartão de confirmação");
        };
        assert!(
            actions[0]
                .description
                .starts_with("Criar a tarefa “Ligar pro João” para")
        );
        assert!(actions[0].description.contains("às 15:00"));
        assert_eq!(
            store.today().unwrap().planned.len(),
            1,
            "nada criado antes do toque"
        );
        resume(&fake, &host, &system, &mut conv, &["c2".into()]).unwrap();
        let planned = store.today().unwrap().planned;
        assert_eq!(planned.len(), 2);
        let diario = store.journal_for_day(today).unwrap();
        assert_eq!(diario.last().unwrap().actor, "assistant");
    }

    #[test]
    fn instrucoes_trazem_o_dia_e_o_jeito_de_citar() {
        let now = Local::now();
        let s = system_prompt(Some("Marcus"), now, true);
        assert!(s.contains(&now.format("%Y-%m-%d").to_string()));
        assert!(s.contains("[[reuniao:42@754]]"));
        assert!(s.contains("opt_time_"));
        assert!(system_prompt(None, now, false).contains("não está conectado"));
    }
}
