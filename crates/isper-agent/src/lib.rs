//! O agente do ISPer (Fase 10.4, [ADR 0021] e [ADR 0023]): perguntas livres
//! sobre o seu dia, o resumo da manhã e o fechamento do dia.
//!
//! O laço é o do `isper-llm` ([`isper_llm::agent`]); aqui ficam as
//! ferramentas ([`IsperHost`]) e o que se pede ao modelo:
//!
//! - as do ISPer, sobre o `isper.db` (tarefas, diário, reuniões, ditados,
//!   rotinas e as sugeridas; criar, mover e concluir tarefa pedem o toque, e
//!   guardar na memória pede sempre);
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
use isper_llm::agent::{Permission, Source, ToolHost, ToolOutcome};
use local::LocalTools;
use serde_json::Value;

pub use opttime::{OptTimeTools, default_or_chosen};

/// Se a ferramenta pede o toque sempre, sem poder ficar livre: guardar na
/// memória ([ADR 0023]). As do OptTime que apagam também, pelas marcações.
///
/// [ADR 0023]: ../../../docs/adr/0023-escada-de-confianca.md
pub fn ask_only(name: &str) -> bool {
    LocalTools::ask_only(name)
}

/// As ferramentas do ISPer com a permissão padrão de cada uma (a tela de
/// permissões lista estas e as do OptTime).
pub fn local_catalog() -> Vec<(ToolSpec, Permission)> {
    LocalTools::specs()
        .into_iter()
        .map(|t| {
            let p = LocalTools::permission(&t.name);
            (t, p)
        })
        .collect()
}

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

    /// A pergunta com os registros que a pessoa anexou (`(tipo, id)`, com
    /// tipo `tarefa`, `reuniao` ou `projeto`), lidos agora do banco no mesmo
    /// formato das ferramentas, e as fontes deles para a resposta citar. Um
    /// registro que não dá para ler vira uma linha dizendo isso, sem derrubar
    /// a pergunta.
    pub fn attach(&self, question: &str, items: &[(String, String)]) -> (String, Vec<Source>) {
        let mut text = question.trim().to_string();
        let mut sources = Vec::new();
        if items.is_empty() {
            return (text, sources);
        }
        text.push_str(
            "\n\nRegistros que anexei (lidos agora do ISPer; cite pelas referências \"ref\"):",
        );
        for (kind, id) in items {
            let out = self.local.attachment(kind, id);
            if out.is_error {
                text.push_str(&format!(
                    "\n- {kind}:{id} — não deu para ler: {}",
                    out.content
                ));
            } else {
                text.push_str("\n- ");
                text.push_str(&out.content);
                sources.extend(out.sources);
            }
        }
        (text, sources)
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
                // Guardar na memória nunca fica livre.
                Some(Permission::Allow) if LocalTools::ask_only(name) => Permission::Ask,
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

/// Como o assistente se apresenta e fala (Configurações → Assistente). Muda
/// só o estilo: as regras do prompt valem do mesmo jeito para qualquer
/// persona. Valor fora da lista vale o padrão.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Persona {
    /// O nome que ele usa; `None` é só "o assistente".
    pub name: Option<String>,
    /// `direct` (padrão) · `warm` · `formal`.
    pub tone: String,
    /// `short` · `normal` (padrão) · `detailed`.
    pub length: String,
}

impl Persona {
    fn style(&self) -> String {
        let tone = match self.tone.as_str() {
            "warm" => {
                "Com calor e leveza, como um colega próximo: pode reconhecer o esforço e encorajar, sem exagero e sem emoji."
            }
            "formal" => {
                "Com cordialidade e formalidade, como um assistente executivo: frases completas, sem gíria."
            }
            _ => "Direto, como um colega que sabe da agenda: sem rodeio nem elogio.",
        };
        let length = match self.length.as_str() {
            "short" => "O mínimo que resolve: sem introdução nem fecho, uma linha quando der.",
            "detailed" => "Pode detalhar: dê o contexto e o porquê que ajudarem, em tópicos.",
            _ => "Curto: tópicos quando houver lista.",
        };
        format!("{tone} {length}")
    }
}

/// As instruções do agente: quem é a pessoa, que horas são, o que ele sabe
/// dela (a memória, que ela vê e edita), como citar e o jeito de falar
/// ([`Persona`]).
pub fn system_prompt(
    me: Option<&str>,
    now: DateTime<Local>,
    with_opttime: bool,
    memories: &[String],
    persona: &Persona,
) -> String {
    let who = me.map(|n| format!(" de {n}")).unwrap_or_default();
    let intro = match persona.name.as_deref() {
        Some(name) => format!(
            "Você é {name}, o assistente pessoal{who} no ISPer, o app de ditado e reuniões dele. Se perguntarem o seu nome, é {name}."
        ),
        None => {
            format!("Você é o assistente pessoal{who} no ISPer, o app de ditado e reuniões dele.")
        }
    };
    let style = persona.style();
    let memory = if memories.is_empty() {
        String::new()
    } else {
        let lines: Vec<String> = memories.iter().map(|m| format!("- {m}")).collect();
        format!(
            "\n\nO que você sabe da pessoa (a memória dela, que ela vê e edita nas Configurações; vale mais que o seu palpite):\n{}",
            lines.join("\n")
        )
    };
    let opttime = if with_opttime {
        "\n- As ferramentas opt_time_* são do OptTime da empresa: agenda do Outlook, horas lançadas, work items do Azure DevOps e sugestões para preencher o dia."
    } else {
        "\n- O OptTime não está conectado: sem agenda do Outlook nem horas. Se a pergunta depender disso, diga."
    };
    format!(
        "{intro} Hoje é {weekday}, {date} ({iso}), {time}.

Como responder:
- Em português. {style}
- O jeito de falar é só estilo: as regras abaixo valem sempre, do mesmo jeito.
- Quando a pessoa anexa registros à pergunta, eles já vêm lidos do ISPer: responda com eles e cite pelas referências deles; busque mais só se faltar.
- Use as ferramentas para buscar os fatos antes de responder. Não invente tarefa, reunião, horário nem número.
- Se faltar detalhe, chame a ferramenta de novo (por exemplo `reuniao` com `busca`) em vez de pedir para a pessoa abrir ou procurar algo.
- Cite a origem de cada fato com a referência que vem nos resultados, entre colchetes duplos, logo depois do fato: [[tarefa:…]], [[reuniao:42@754]], [[opttime:…]]. Só cite referências que vieram das ferramentas.
- \"Hoje\" é {iso}; \"ontem\" e \"amanhã\" contam a partir daí. Datas nas ferramentas vão como AAAA-MM-DD.
- Para criar, mover ou concluir tarefa e para lançar horas, chame a ferramenta: o ISPer mostra um cartão e a pessoa confirma. Não peça confirmação em texto e não diga que fez antes do resultado.
- Se uma ferramenta falhar ou for recusada, diga o que faltou em uma linha.
- Se a pessoa pedir para lembrar algo, ou disser um fato que vale para os próximos dias (quem é o gestor, um jeito de preferir), proponha guardar com `lembrar`. Nunca senha, segredo ou o que outra pessoa disse numa reunião.
- Para tempo livre ou \"o que dá para fazer/estudar\", use `quando_der_tempo` (as listas sem data); sugira um item que caiba, não crie tarefa sem a pessoa pedir.
- Quando a pergunta é sobre um projeto (\"o que tenho do Portal?\"), filtre `tarefas_do_dia` e `reunioes` por `projeto`; na dúvida do nome, veja `projetos`.
- Rotinas sugeridas (em `rotinas`) são só sugestões: conte, com a evidência, e diga que dá para aceitar na tela Hoje. Você não cria rotina.{opttime}{memory}",
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
        let mut chosen = HashMap::new();
        chosen.insert("lembrar".to_string(), Permission::Allow);
        let host = IsperHost::new(&path, None, chosen);
        assert_eq!(
            host.permission("lembrar"),
            Permission::Ask,
            "guardar na memória nunca fica livre"
        );
        assert!(ask_only("lembrar") && !ask_only("criar_tarefa"));
    }

    #[test]
    fn filtra_as_tarefas_por_projeto_e_diz_quais_existem() {
        use isper_assist::NewProject;
        let path = db();
        let store = AssistStore::open(&path).unwrap();
        let portal = store
            .create_project(
                NewProject {
                    name: "Portal".into(),
                    aliases: vec!["PRT".into()],
                    ..NewProject::default()
                },
                Actor::User,
            )
            .unwrap();
        let today = store.clock().today();
        for (title, project) in [
            ("Revisar o PR", Some(portal.id.clone())),
            ("Pagar o boleto", None),
        ] {
            let task = NewTask {
                planned_on: Some(today),
                project_id: project,
                ..NewTask::titled(title)
            };
            store.create_task(task, Actor::User).unwrap();
        }
        let tools = LocalTools::new(&path);
        let out = tools.call("tarefas_do_dia", &json!({"dia": null, "projeto": "prt"}));
        assert!(!out.is_error, "{}", out.content);
        assert!(
            out.content.contains("Revisar o PR") && out.content.contains("\"projeto\":\"Portal\"")
        );
        assert!(
            !out.content.contains("Pagar o boleto"),
            "o filtro tira a sem projeto"
        );
        let none = tools.call("tarefas_do_dia", &json!({"dia": null, "projeto": "mrp"}));
        assert!(
            none.is_error && none.content.contains("Portal"),
            "{}",
            none.content
        );
    }

    #[test]
    fn lembrar_guarda_depois_do_toque_e_rotinas_traz_as_sugeridas() {
        let path = db();
        let host = IsperHost::new(&path, None, HashMap::new());
        let fake = FakeChat::new(vec![
            call(
                "m1",
                "lembrar",
                json!({"texto": "Meu gestor é o Carlos", "tipo": "fato"}),
            ),
            says("Guardei."),
            call("r1", "rotinas", json!({})),
            says("Nenhuma sugerida."),
        ]);
        let system = system_prompt(None, Local::now(), false, &[], &Persona::default());
        let mut conv = Conversation::new("lembra que meu gestor é o Carlos");
        let Step::Confirm { actions } = advance(&fake, &host, &system, &mut conv).unwrap() else {
            panic!("guardar pede o toque");
        };
        assert_eq!(
            actions[0].description,
            "Guardar na memória “Meu gestor é o Carlos”"
        );
        let store = AssistStore::open(&path).unwrap();
        assert!(
            store.memories(false).unwrap().is_empty(),
            "nada antes do toque"
        );
        resume(&fake, &host, &system, &mut conv, &["m1".into()]).unwrap();
        let saved = store.memories(false).unwrap();
        assert_eq!(saved[0].text, "Meu gestor é o Carlos");
        assert_eq!(saved[0].origin, "assistant");

        conv.ask("tem rotina sugerida?");
        advance(&fake, &host, &system, &mut conv).unwrap();
        let sent = fake.messages(3);
        let isper_llm::ChatMessage::Tools { results } = sent.last().unwrap() else {
            panic!("esperava o resultado de rotinas");
        };
        assert!(results[0].content.contains("\"sugeridas\":[]"));
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
        let system = system_prompt(
            Some("Marcus"),
            Local::now(),
            false,
            &[],
            &Persona::default(),
        );
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
        let s = system_prompt(Some("Marcus"), now, true, &[], &Persona::default());
        assert!(s.contains(&now.format("%Y-%m-%d").to_string()));
        assert!(s.contains("[[reuniao:42@754]]"));
        assert!(s.contains("opt_time_"));
        assert!(!s.contains("O que você sabe da pessoa"));
        assert!(
            system_prompt(None, now, false, &[], &Persona::default())
                .contains("não está conectado")
        );
        let s = system_prompt(
            None,
            now,
            false,
            &["Meu gestor é o Carlos".into()],
            &Persona::default(),
        );
        assert!(s.ends_with("- Meu gestor é o Carlos"));
    }

    #[test]
    fn persona_muda_o_nome_e_o_jeito_sem_mexer_nas_regras() {
        let now = Local::now();
        let base = system_prompt(None, now, false, &[], &Persona::default());
        assert!(base.starts_with("Você é o assistente pessoal"));
        assert!(base.contains("Direto,"));
        let lia = Persona {
            name: Some("Lia".into()),
            tone: "warm".into(),
            length: "short".into(),
        };
        let s = system_prompt(None, now, false, &[], &lia);
        assert!(s.starts_with("Você é Lia, o assistente pessoal"));
        assert!(s.contains("seu nome, é Lia"));
        assert!(s.contains("Com calor") && s.contains("O mínimo que resolve"));
        assert!(!s.contains("Direto,"));
        let rules = |x: &str| x[x.find("- O jeito de falar").unwrap()..].to_string();
        assert_eq!(rules(&base), rules(&s), "as regras são as mesmas");
        let odd = Persona {
            tone: "pirata".into(),
            length: "enorme".into(),
            ..Persona::default()
        };
        assert_eq!(
            system_prompt(None, now, false, &[], &odd),
            base,
            "valor fora da lista vale o padrão"
        );
    }

    #[test]
    fn anexos_vao_lidos_na_pergunta_e_a_resposta_cita_sem_buscar() {
        use isper_assist::NewProject;
        let path = db();
        let store = AssistStore::open(&path).unwrap();
        let project = |name: &str| {
            store
                .create_project(
                    NewProject {
                        name: name.into(),
                        ..NewProject::default()
                    },
                    Actor::User,
                )
                .unwrap()
        };
        let portal = project("Portal");
        let cliente = project("Portal do Cliente");
        let task = |title: &str, notes: &str, project: &str| {
            store
                .create_task(
                    NewTask {
                        notes: notes.into(),
                        project_id: Some(project.to_string()),
                        ..NewTask::titled(title)
                    },
                    Actor::User,
                )
                .unwrap()
        };
        let t = task("Revisar o contrato", "Ver a cláusula 4", &portal.id);
        task("Deploy do cliente", "", &cliente.id);
        let host = IsperHost::new(&path, None, HashMap::new());
        let (text, sources) = host.attach(
            " o que falta aqui? ",
            &[
                ("tarefa".into(), t.id.clone()),
                ("projeto".into(), portal.id.clone()),
                ("reuniao".into(), "999".into()),
            ],
        );
        assert!(text.starts_with("o que falta aqui?\n\nRegistros que anexei"));
        assert!(
            text.contains("Ver a cláusula 4"),
            "a tarefa vai inteira, com as notas"
        );
        assert!(text.contains("\"projeto\":\"Portal\""));
        assert!(
            !text.contains("Deploy do cliente"),
            "o projeto vai pelo id, não pelo nome parecido"
        );
        assert!(
            text.contains("reuniao:999 — não deu para ler"),
            "o que não dá para ler vira uma linha, sem derrubar a pergunta"
        );
        assert_eq!(host.attach("só isso", &[]), ("só isso".into(), Vec::new()));

        let fake = FakeChat::new(vec![says(&format!(
            "Falta a cláusula 4 [[tarefa:{}]].",
            t.id
        ))]);
        let system = system_prompt(None, Local::now(), false, &[], &Persona::default());
        let mut conv = Conversation::new(&text);
        conv.add_sources(sources);
        let Step::Answer { sources, .. } = advance(&fake, &host, &system, &mut conv).unwrap()
        else {
            panic!("esperava resposta");
        };
        assert_eq!(
            sources.len(),
            1,
            "a citação do anexo vira fonte sem ferramenta"
        );
        assert_eq!(sources[0].label, "Revisar o contrato");
    }
}
