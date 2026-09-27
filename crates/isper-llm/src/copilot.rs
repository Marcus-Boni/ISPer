//! Copilot Executivo em tempo real (Fase 8):
//!
//! Acompanha a reunião em andamento com foco em DECISÃO e AÇÃO:
//! - Detecta acordos firmados e decisões de negócio;
//! - Extrai tarefas com responsável e prazo (alertando quando falta prazo);
//! - Sinaliza objeções ou riscos não tratados pelos participantes;
//! - Sugere perguntas estratégicas que o usuário ("Eu") deve fazer;
//! - Permite Q&A in-meeting ("Pergunte à Reunião") e enriquecimento de notas (estilo Granola).
//!
//! Dois detalhes de projeto que valem a leitura:
//!
//! 1. **O `id` do card é nosso, não do modelo.** A IA devolve `"c1"`, `"c2"`
//!    a cada rodada — ids que colidem entre rodadas e fariam o botão
//!    "Confirmar" de um card mexer em outro. Aqui o id sai de
//!    `kind + título normalizado` ([`card_id`]), então o mesmo assunto cai
//!    sempre no mesmo id e a mesclagem entre rodadas é natural.
//!
//! 2. **O gatilho barato vem antes do caro.** [`detect_trigger`] roda local,
//!    em cima do texto recém-transcrito, e só então a rodada de LLM é
//!    antecipada. É o que faz a decisão aparecer em segundos em vez de
//!    esperar o pulso periódico. Só que as frases fixas pegam 5% dos momentos
//!    medidos numa amostra de reuniões reais; com a chave da TypeSafe, o
//!    filtro do Jev ([`filter_questions`], [`read_filter`]) lê cada parágrafo
//!    e pega 92% — e o LLM passa a olhar só os trechos marcados.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Result;
use crate::providers::LlmProvider;

/// Tipo de card de inteligência em tempo real.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardKind {
    /// Decisão ou acordo firmado na conversa.
    Decision,
    /// Tarefa com responsável e prazo.
    Action,
    /// Risco, objeção de participante ou incoerência identificada.
    Risk,
    /// Pergunta estratégica sugerida para o usuário fazer.
    Question,
}

impl CardKind {
    pub fn label_pt(&self) -> &'static str {
        match self {
            Self::Decision => "Decisão",
            Self::Action => "Ação",
            Self::Risk => "Alerta de Risco",
            Self::Question => "Pergunta Recomendada",
        }
    }

    /// Nome estável do tipo — o mesmo que sai no JSON e vai para o banco.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Decision => "decision",
            Self::Action => "action",
            Self::Risk => "risk",
            Self::Question => "question",
        }
    }

    /// Prefixo curto do id (o id precisa caber num `data-` de HTML sem susto).
    fn slug(&self) -> &'static str {
        match self {
            Self::Decision => "dec",
            Self::Action => "act",
            Self::Risk => "rsk",
            Self::Question => "qst",
        }
    }
}

/// Nível de relevância ou urgência do card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CardUrgency {
    Low,
    #[default]
    Medium,
    High,
}

impl CardUrgency {
    /// Nome estável — o mesmo que sai no JSON e vai para o banco.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

/// Estado do card na reunião (pode ser validado ou descartado pelo usuário).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CardStatus {
    #[default]
    Proposed,
    Confirmed,
    Discarded,
}

/// Um card de inteligência individual gerado pelo Copilot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopilotCard {
    pub id: String,
    pub kind: CardKind,
    pub title: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    pub urgency: CardUrgency,
    pub at_secs: u32,
    pub status: CardStatus,
}

impl CopilotCard {
    /// Ação sem prazo combinado: o Copilot cobra isso antes da reunião acabar.
    pub fn missing_due_date(&self) -> bool {
        self.kind == CardKind::Action
            && self
                .due_date
                .as_deref()
                .map(|d| {
                    let n = normalize(d);
                    n.is_empty() || n.contains("sem prazo") || n.contains("nao definido")
                })
                .unwrap_or(true)
    }
}

/// Resultado de uma rodada de análise cognitiva do Copilot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CopilotAnalysis {
    /// Tópico corrente sendo discutido.
    pub current_topic: String,
    /// Novos cards identificados ou atualizados nesta rodada.
    pub cards: Vec<CopilotCard>,
    /// Observação de dinâmica (ex.: alerta de monólogo, ritmo da reunião).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dynamics_note: Option<String>,
}

/// Insumo para a rodada de análise cognitiva.
pub struct CopilotInput<'a> {
    pub window: &'a str,
    pub elapsed: &'a str,
    pub elapsed_secs: u32,
    pub previous_cards: &'a [CopilotCard],
    /// Trechos que o filtro ([`read_filter`]) marcou nesta janela. Vazio na
    /// rodada comum; na rodada focada, diz à IA onde olhar primeiro.
    pub focus: &'a [FocusHint],
}

/// Um parágrafo marcado pelo filtro: quando começou e o tipo provável.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FocusHint {
    pub at_secs: f32,
    pub kind: CardKind,
}

// ---------------------------------------------------------------- Normalização

/// Dobra acentos do pt-BR e baixa a caixa: "Ação Não Definida" → "acao nao definida".
/// Serve para comparar títulos entre rodadas sem depender de acento nem de caixa.
fn fold_char(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ã' | 'ä' | 'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => 'a',
        'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'i',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'u',
        'ç' | 'Ç' => 'c',
        'ñ' | 'Ñ' => 'n',
        other => other.to_ascii_lowercase(),
    }
}

/// Texto comparável: sem acento, minúsculo, só alfanumérico, espaço único.
pub(crate) fn normalize(s: &str) -> String {
    let folded: String = s
        .chars()
        .map(fold_char)
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Id estável de um card: mesmo assunto ⇒ mesmo id, rodada após rodada.
///
/// É o que permite mesclar sem duplicar e garante que "Confirmar" mexa no
/// card certo — o id que a IA inventa (`"c1"`) colide entre rodadas.
pub fn card_id(kind: CardKind, title: &str) -> String {
    let slug: String = normalize(title)
        .split_whitespace()
        .take(6)
        .collect::<Vec<_>>()
        .join("-");
    let slug: String = if slug.is_empty() {
        "card".to_string()
    } else {
        slug.chars().take(48).collect()
    };
    format!("{}-{slug}", kind.slug())
}

/// Semelhança de Jaccard entre os tokens de dois títulos (0.0 a 1.0).
///
/// A IA reformula o mesmo ponto entre rodadas ("Lançamento no dia 30" →
/// "Lançamento dia 30"). Comparar palavra a palavra evita dois cards para
/// o mesmo assunto sem exigir título idêntico.
pub(crate) fn title_similarity(a: &str, b: &str) -> f32 {
    let ta: Vec<String> = normalize(a)
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let tb: Vec<String> = normalize(b)
        .split_whitespace()
        .map(str::to_string)
        .collect();
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let inter = ta.iter().filter(|t| tb.contains(t)).count();
    let union = ta.len() + tb.len() - inter;
    if union == 0 {
        return 0.0;
    }
    inter as f32 / union as f32
}

/// Acima disto, dois títulos falam do mesmo assunto e viram um card só.
pub const SAME_CARD_SIMILARITY: f32 = 0.7;

/// Dois cards tratam do mesmo ponto? (mesmo tipo e título equivalente)
pub fn is_same_card(a: &CopilotCard, b: &CopilotCard) -> bool {
    a.kind == b.kind
        && (a.id == b.id || title_similarity(&a.title, &b.title) >= SAME_CARD_SIMILARITY)
}

// ---------------------------------------------------------------- Gatilhos locais

/// O que o texto recém-transcrito sugere que acabou de acontecer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerKind {
    /// "fechado", "combinado", "vamos seguir com" — acordo sendo firmado.
    Decision,
    /// "eu envio", "fico de", "vai avaliar" — tarefa sendo atribuída.
    Action,
    /// "discordo", "me preocupa", "tem um risco" — objeção na mesa.
    Risk,
}

impl TriggerKind {
    /// Nome estável ("decision", "action", "risk") — é o que vai para a UI,
    /// que mostra a frase no idioma da interface.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Decision => "decision",
            Self::Action => "action",
            Self::Risk => "risk",
        }
    }

    /// Rótulo curto para o log ("o que acordou o Copilot").
    pub fn label_pt(&self) -> &'static str {
        match self {
            Self::Decision => "acordo detectado",
            Self::Action => "tarefa atribuída",
            Self::Risk => "objeção levantada",
        }
    }
}

/// Frases que denunciam cada tipo de momento, em pt-BR falado.
///
/// Tudo já normalizado (sem acento, minúsculo): a comparação roda sobre
/// [`normalize`], então não há variante acentuada aqui.
const DECISION_CUES: &[&str] = &[
    "fechado",
    "fechamos",
    "combinado",
    "fica decidido",
    "ficou decidido",
    "vamos seguir com",
    "vamos seguir",
    "esta aprovado",
    "aprovado entao",
    "entao fica",
    "de acordo entao",
    "acordado",
    "decidido entao",
    "pode seguir",
    "alinhado entao",
];

const ACTION_CUES: &[&str] = &[
    "eu envio",
    "eu mando",
    "vou enviar",
    "vou mandar",
    "vou preparar",
    "fico de",
    "fica de",
    "vai avaliar",
    "vai verificar",
    "vai preparar",
    "fica responsavel",
    "assume isso",
    "ate sexta",
    "ate amanha",
    "ate segunda",
    "semana que vem",
];

const RISK_CUES: &[&str] = &[
    "discordo",
    "nao concordo",
    "me preocupa",
    "preocupacao",
    "tem um risco",
    "e arriscado",
    "o problema e",
    "nao vai dar",
    "impedimento",
    "bloqueio",
    "receio",
    "nao esta claro",
    "duvida sobre",
    "nao faz sentido",
];

/// Detecta, sem rede e sem custo, se o trecho recém-falado merece uma rodada
/// imediata de análise. `None` = nada de especial, o pulso periódico basta.
pub fn detect_trigger(text: &str) -> Option<TriggerKind> {
    let n = normalize(text);
    if n.is_empty() {
        return None;
    }
    let hit = |cues: &[&str]| cues.iter().any(|c| n.contains(c));
    // Ordem = prioridade: decisão e ação valem mais que uma objeção solta.
    if hit(DECISION_CUES) {
        Some(TriggerKind::Decision)
    } else if hit(ACTION_CUES) {
        Some(TriggerKind::Action)
    } else if hit(RISK_CUES) {
        Some(TriggerKind::Risk)
    } else {
        None
    }
}

// ---------------------------------------------------------------- Filtro (Jev)

/// A partir desta probabilidade de "merece card", o parágrafo vai para o LLM.
///
/// Medido nos 450 parágrafos reais (estudo, §12.5): a 0,5, o filtro pega 92%
/// dos momentos que viram card e manda só 45% dos parágrafos adiante. A
/// precisão ali é 0,49 — por isso ele filtra e o LLM decide, nunca o
/// contrário.
pub const FILTER_THRESHOLD: f32 = 0.5;

/// Parágrafo com menos palavras que isto nem vai ao filtro: "uhum", "perfeito,
/// beleza" nunca viram card (o corpus da medição usou o mesmo corte).
pub const FILTER_MIN_WORDS: usize = 8;

const FILTER_KIND: &str = "kind";

/// A pergunta do filtro, exatamente como foi medida (pt-BR, a ordem das
/// opções incluída). Mudar o texto é mudar o modelo: meça de novo.
pub fn filter_questions() -> Vec<(&'static str, crate::systemone::Question)> {
    let opt = |k: &str, v: &str| (k.to_string(), v.to_string());
    vec![(
        FILTER_KIND,
        crate::systemone::Question::Choice {
            instructions: "Este é um parágrafo da transcrição automática de uma reunião de trabalho (pode ter erros de transcrição). 'anterior' é o parágrafo antes dele, só para contexto. Classifique o que o parágrafo 'atual' faz.".into(),
            options: vec![
                opt("decision", "uma decisão ou acordo é firmado aqui: concluem como algo vai ficar (ex.: 'então a gente deixa assim', 'a estrutura já está certa')"),
                opt("action", "uma tarefa para depois deste momento: alguém se compromete a fazer algo ou pede que alguém faça ou mude algo (ex.: 'vou te mandar', 'você consegue ver', 'mostra o ano aqui')"),
                opt("risk", "um problema, defeito, bloqueio, objeção ou dúvida sem resposta sobre o plano"),
                opt("none", "explicação, demonstração narrada, confirmação, conversa social ou fragmento sem decisão, tarefa ou problema"),
            ],
        },
    )]
}

/// O `state` do filtro: o parágrafo e o anterior, cada um como
/// `"Falante: texto"`. O anterior é só contexto — a pergunta diz isso.
pub fn filter_state(previous: &str, current: &str) -> Value {
    serde_json::json!({ "anterior": previous, "atual": current })
}

/// O que o filtro achou de um parágrafo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilterVerdict {
    /// 1 − p(nada): a chance de o parágrafo merecer um card.
    pub p_card: f32,
    /// O tipo mais provável entre decisão, ação e risco.
    pub kind: CardKind,
    /// `p_card` passou do [`FILTER_THRESHOLD`].
    pub flagged: bool,
}

/// Lê a resposta do filtro. `None` se ela não tiver a pergunta do filtro.
pub fn read_filter(decision: &crate::systemone::Decision) -> Option<FilterVerdict> {
    let answer = decision.answer(FILTER_KIND)?;
    let p = |k: &str| answer.probability(k).unwrap_or(0.0);
    let p_card = (1.0 - p("none")).clamp(0.0, 1.0);
    let kind = [
        (CardKind::Decision, p("decision")),
        (CardKind::Action, p("action")),
        (CardKind::Risk, p("risk")),
    ]
    .into_iter()
    .max_by(|a, b| a.1.total_cmp(&b.1))
    .map_or(CardKind::Action, |(k, _)| k);
    Some(FilterVerdict {
        p_card,
        kind,
        flagged: p_card >= FILTER_THRESHOLD,
    })
}

/// `mm:ss` (ou `h:mm:ss`) para o prompt — o mesmo formato da janela.
fn clock(secs: f32) -> String {
    let s = secs.max(0.0) as u32;
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

// ---------------------------------------------------------------- Análise

const SYSTEM_COPILOT: &str = r#"Você é o ISPer Copilot, um assistente executivo e estrategista de reuniões de alto nível.
Sua missão é atuar como segundo cérebro do usuário ("Eu"), identificando com precisão absoluta:
1. DECISÕES firmadas (acordos, consensos ou deliberações claras);
2. AÇÕES (tarefas combinadas, quem é o responsável e qual o prazo — marque explicitamente quando faltar prazo!);
3. RISCOS e OBJEÇÕES (dúvidas levantadas pelos participantes, hesitações ou divergências não resolvidas);
4. PERGUNTAS ESTRATÉGICAS (o que o usuário "Eu" deveria perguntar agora para evitar problemas futuros).

REGRAS RÍGIDAS:
- Sinal, não ruído: prefira devolver NENHUM card a devolver um card óbvio ou genérico.
- Um card só existe se algo concreto foi dito. Não invente fatos, nomes, prazos ou números.
- Seja ultra-conciso, pragmático e profissional.
- Responda SEMPRE em JSON válido, sem markdown ao redor.
- Em pt-BR natural e executivo."#;

/// Monta o prompt para a análise estruturada em JSON.
pub(crate) fn build_analysis_prompt(input: &CopilotInput<'_>) -> String {
    let existing_summary: Vec<String> = input
        .previous_cards
        .iter()
        .filter(|c| c.status != CardStatus::Discarded)
        .map(|c| format!("- [{}] {}: {}", c.kind.label_pt(), c.title, c.description))
        .collect();

    let existing_block = if existing_summary.is_empty() {
        "Nenhum card registrado até agora.".to_string()
    } else {
        format!(
            "Cards já identificados anteriormente (NÃO repita nenhum destes; devolva apenas o que for NOVO ou tiver detalhe novo — repetir o mesmo ponto com outras palavras é erro):\n{}",
            existing_summary.join("\n")
        )
    };

    let focus_block = if input.focus.is_empty() {
        String::new()
    } else {
        let lines: Vec<String> = input
            .focus
            .iter()
            .map(|f| format!("- [{}] provável {}", clock(f.at_secs), f.kind.label_pt()))
            .collect();
        format!(
            "\n\nUm filtro rápido marcou estes pontos do trecho como prováveis momentos importantes. Confira cada um na conversa: se não houver nada concreto, não crie card.\n{}",
            lines.join("\n")
        )
    };

    format!(
        r#"A reunião está em {elapsed}. "Eu" é quem opera o assistente; "Participantes" são os demais interlocutores.

{existing_block}

Trecho recente da conversa:
---
{window}
---{focus_block}

Retorne um objeto JSON com a seguinte estrutura:
{{
  "current_topic": "resumo de 1 frase curta sobre o assunto do momento",
  "cards": [
    {{
      "kind": "decision" | "action" | "risk" | "question",
      "title": "título curto e direto (máx 8 palavras)",
      "description": "detalhe objetivo do ponto levantado",
      "owner": "nome da pessoa ou 'Eu' (se for ação) ou null",
      "due_date": "data/prazo explícito ou 'Sem prazo definido' (se for ação) ou null",
      "urgency": "low" | "medium" | "high"
    }}
  ],
  "dynamics_note": "dica breve de dinâmica se relevante (ex: 'Ponto crucial: esclareça o prazo antes de mudar de assunto') ou null"
}}

Se nada de novo e concreto apareceu no trecho, devolva "cards": []."#,
        elapsed = input.elapsed,
        window = input.window.trim(),
    )
}

/// Executa a rodada de análise cognitiva e devolve os cards estruturados.
pub fn analyze_meeting(
    provider: &dyn LlmProvider,
    input: &CopilotInput<'_>,
) -> Result<CopilotAnalysis> {
    let prompt = build_analysis_prompt(input);
    let raw = provider.complete(SYSTEM_COPILOT, &prompt)?;
    parse_analysis_json(&raw, input.elapsed_secs)
}

/// Extrai e normaliza o JSON mesmo quando o LLM coloca cercas ```json ... ```
pub(crate) fn parse_analysis_json(raw: &str, default_at_secs: u32) -> Result<CopilotAnalysis> {
    let text = sanitize_json(raw);
    let val: Value = serde_json::from_str(&text)
        .map_err(|e| crate::LlmError::BadResponse(format!("JSON inválido da IA: {e}")))?;

    let current_topic = val["current_topic"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "null")
        .unwrap_or("Em andamento")
        .to_string();

    let dynamics_note = val["dynamics_note"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "null")
        .map(str::to_string);

    let mut cards: Vec<CopilotCard> = Vec::new();
    if let Some(arr) = val["cards"].as_array() {
        for item in arr {
            let kind = match item["kind"].as_str() {
                Some("decision") => CardKind::Decision,
                Some("action") => CardKind::Action,
                Some("risk") => CardKind::Risk,
                Some("question") => CardKind::Question,
                _ => continue,
            };

            let title = item["title"].as_str().unwrap_or("").trim().to_string();
            if title.is_empty() {
                continue;
            }

            let text_field = |key: &str| {
                item[key]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty() && *s != "null")
                    .map(str::to_string)
            };

            let urgency = match item["urgency"].as_str() {
                Some("high") => CardUrgency::High,
                Some("low") => CardUrgency::Low,
                _ => CardUrgency::Medium,
            };

            let card = CopilotCard {
                // O id é nosso: o da IA colide entre rodadas (ver doc do módulo).
                id: card_id(kind, &title),
                kind,
                title,
                description: text_field("description").unwrap_or_default(),
                owner: text_field("owner"),
                due_date: text_field("due_date"),
                urgency,
                at_secs: default_at_secs,
                status: CardStatus::Proposed,
            };

            // A própria rodada às vezes repete o ponto em dois cards.
            if cards.iter().any(|c| is_same_card(c, &card)) {
                continue;
            }
            cards.push(card);
        }
    }

    Ok(CopilotAnalysis {
        current_topic,
        cards,
        dynamics_note,
    })
}

// ---------------------------------------------------------------- Ata final

/// Seção de Markdown com o que o usuário validou durante a reunião.
///
/// É o que fecha o ciclo: sem isso, confirmar um card no HUD não deixa rastro
/// nenhum no arquivo da reunião. `None` quando nada foi confirmado.
pub fn render_decisions_markdown(cards: &[CopilotCard]) -> Option<String> {
    let confirmed: Vec<&CopilotCard> = cards
        .iter()
        .filter(|c| c.status == CardStatus::Confirmed)
        .collect();
    if confirmed.is_empty() {
        return None;
    }

    let mut out = String::from("## Decisões e ações validadas no Copilot\n\n");
    out.push_str("_Confirmadas por você durante a reunião, ao vivo._\n");

    let mut section = |kind: CardKind, heading: &str| {
        let items: Vec<&&CopilotCard> = confirmed.iter().filter(|c| c.kind == kind).collect();
        if items.is_empty() {
            return;
        }
        out.push_str(&format!("\n### {heading}\n\n"));
        for c in items {
            out.push_str(&format!("- **{}**", c.title.trim()));
            if !c.description.trim().is_empty() {
                out.push_str(&format!(" — {}", c.description.trim()));
            }
            let mut meta: Vec<String> = Vec::new();
            if let Some(o) = c.owner.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                meta.push(format!("responsável: {o}"));
            }
            if let Some(d) = c
                .due_date
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                meta.push(format!("prazo: {d}"));
            }
            if !meta.is_empty() {
                out.push_str(&format!(" ({})", meta.join(" · ")));
            }
            out.push('\n');
        }
    };

    section(CardKind::Decision, "Decisões");
    section(CardKind::Action, "Ações");
    section(CardKind::Risk, "Riscos e objeções em aberto");
    section(CardKind::Question, "Perguntas a retomar");

    let pendentes: Vec<&&CopilotCard> = confirmed.iter().filter(|c| c.missing_due_date()).collect();
    if !pendentes.is_empty() {
        out.push_str("\n> **Prazo em aberto** em: ");
        out.push_str(
            &pendentes
                .iter()
                .map(|c| c.title.trim())
                .collect::<Vec<_>>()
                .join("; "),
        );
        out.push_str(".\n");
    }

    Some(out)
}

// ---------------------------------------------------------------- Q&A In-Meeting

const SYSTEM_QA: &str = r#"Você é o assistente executivo in-meeting do usuário.
Ele está em uma reunião agora e precisa de respostas rápidas, diretas e de alto valor.
Responda em pt-BR de forma clara, em até 3 ou 4 tópicos curtos.
Use EXCLUSIVAMENTE o conteúdo da transcrição fornecida. Se a informação não foi mencionada, diga claramente que não foi falado."#;

fn qa_prompt(transcript_window: &str, question: &str) -> String {
    format!(
        "Transcrição recente da reunião:\n---\n{}\n---\n\nPergunta do usuário:\n{}",
        transcript_window.trim(),
        question.trim()
    )
}

/// Responde a uma pergunta do usuário sobre a reunião em andamento.
pub fn query_meeting(
    provider: &dyn LlmProvider,
    transcript_window: &str,
    question: &str,
) -> Result<String> {
    query_meeting_stream(provider, transcript_window, question, &mut |_| {})
}

/// Como [`query_meeting`], mas entregando a resposta conforme ela sai do
/// modelo — numa reunião, esperar o parágrafo inteiro é esperar demais.
///
/// O texto devolvido no fim vem aparado; o que passa por `on_token` é cru,
/// porque aparar pedaço a pedaço comeria os espaços entre eles.
pub fn query_meeting_stream(
    provider: &dyn LlmProvider,
    transcript_window: &str,
    question: &str,
    on_token: &mut dyn FnMut(&str),
) -> Result<String> {
    let user_prompt = qa_prompt(transcript_window, question);
    let reply = provider.complete_stream(SYSTEM_QA, &user_prompt, on_token)?;
    Ok(reply.trim().to_string())
}

// ---------------------------------------------------------------- Granola-style Notes

const SYSTEM_NOTES: &str = r#"Você é um sintetizador de notas executivas (estilo Granola).
O usuário digitou anotações rápidas/soltas durante a reunião.
Sua missão é enriquecer essas notas utilizando os fatos, números, prazos e citações EXATAS que constam na transcrição.
Mantenha a estrutura e a ordem dos itens do usuário, mas torne cada ponto completo, profissional e enriquecido com os detalhes falados.
Não invente nada que não esteja na transcrição: se um item do usuário não foi discutido, mantenha-o como está e marque com "(não discutido)"."#;

/// Enriquece as anotações do usuário com os detalhes reais da transcrição.
pub fn enrich_notes(
    provider: &dyn LlmProvider,
    transcript_window: &str,
    raw_notes: &str,
) -> Result<String> {
    enrich_notes_stream(provider, transcript_window, raw_notes, &mut |_| {})
}

/// Como [`enrich_notes`], mas o texto reescrito vai aparecendo enquanto o
/// modelo escreve.
pub fn enrich_notes_stream(
    provider: &dyn LlmProvider,
    transcript_window: &str,
    raw_notes: &str,
    on_token: &mut dyn FnMut(&str),
) -> Result<String> {
    let user_prompt = format!(
        "Transcrição da reunião:\n---\n{}\n---\n\nAnotações brutas do usuário:\n---\n{}\n---\n\nReescreva as anotações enriquecendo cada item com os dados exatos da transcrição em Markdown claro:",
        transcript_window.trim(),
        raw_notes.trim()
    );
    let reply = provider.complete_stream(SYSTEM_NOTES, &user_prompt, on_token)?;
    Ok(reply.trim().to_string())
}

/// Remove cercas Markdown de blocos JSON (` ```json ... ``` `) e sobras de
/// conversa em volta ("Claro! Aqui está: {…}").
fn sanitize_json(raw: &str) -> String {
    let mut s = raw.trim();
    if s.starts_with("```")
        && let Some(pos) = s.find('\n')
    {
        s = &s[pos + 1..];
    }
    if s.ends_with("```")
        && let Some(pos) = s.rfind("```")
    {
        s = &s[..pos];
    }
    let s = s.trim();
    if !s.starts_with('{')
        && let (Some(a), Some(b)) = (s.find('{'), s.rfind('}'))
        && a < b
    {
        return s[a..=b].to_string();
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeProvider;

    fn card(kind: CardKind, title: &str, status: CardStatus) -> CopilotCard {
        CopilotCard {
            id: card_id(kind, title),
            kind,
            title: title.into(),
            description: "detalhe".into(),
            owner: None,
            due_date: None,
            urgency: CardUrgency::Medium,
            at_secs: 10,
            status,
        }
    }

    #[test]
    fn parse_json_valido_com_decisao_e_acao() {
        let json_raw = r#"{
            "current_topic": "Definição do prazo de entrega",
            "cards": [
                {
                    "id": "c1",
                    "kind": "decision",
                    "title": "Lançamento no dia 30",
                    "description": "Ficou acordado com o cliente que a entrega será no dia 30.",
                    "urgency": "high"
                },
                {
                    "id": "c2",
                    "kind": "action",
                    "title": "Enviar proposta revisada",
                    "description": "Preparar PDF e enviar por email",
                    "owner": "Eu",
                    "due_date": "Sexta-feira 18h",
                    "urgency": "medium"
                },
                {
                    "id": "c3",
                    "kind": "risk",
                    "title": "Dúvida sobre suporte",
                    "description": "O cliente mencionou receio com SLA no fim de semana",
                    "urgency": "high"
                }
            ],
            "dynamics_note": "Atenção: confirme o SLA antes de fechar a chamada."
        }"#;

        let res = parse_analysis_json(json_raw, 120).unwrap();
        assert_eq!(res.current_topic, "Definição do prazo de entrega");
        assert_eq!(res.cards.len(), 3);
        assert_eq!(res.cards[0].kind, CardKind::Decision);
        assert_eq!(res.cards[1].kind, CardKind::Action);
        assert_eq!(res.cards[1].owner.as_deref(), Some("Eu"));
        assert_eq!(res.cards[1].due_date.as_deref(), Some("Sexta-feira 18h"));
        assert_eq!(res.cards[2].kind, CardKind::Risk);
        assert_eq!(res.cards[2].urgency, CardUrgency::High);
        assert!(res.dynamics_note.unwrap().contains("confirme o SLA"));
    }

    #[test]
    fn id_do_card_ignora_o_da_ia_e_e_estavel_entre_rodadas() {
        // A IA manda "c1" nas duas rodadas para cards DIFERENTES: se
        // confiássemos nela, "Confirmar" mexeria no card errado.
        let r1 = parse_analysis_json(
            r#"{"current_topic":"t","cards":[
                {"id":"c1","kind":"decision","title":"Lançamento no dia 30","description":"a"}]}"#,
            10,
        )
        .unwrap();
        let r2 = parse_analysis_json(
            r#"{"current_topic":"t","cards":[
                {"id":"c1","kind":"action","title":"Enviar contrato","description":"b"}]}"#,
            20,
        )
        .unwrap();
        assert_ne!(r1.cards[0].id, r2.cards[0].id);

        // E o mesmo assunto, reprocessado, cai sempre no mesmo id.
        let r3 = parse_analysis_json(
            r#"{"current_topic":"t","cards":[
                {"id":"c9","kind":"decision","title":"Lançamento no dia 30","description":"a"}]}"#,
            99,
        )
        .unwrap();
        assert_eq!(r1.cards[0].id, r3.cards[0].id);
        assert!(r1.cards[0].id.starts_with("dec-"));
    }

    #[test]
    fn rodada_repetida_nao_vira_dois_cards() {
        let res = parse_analysis_json(
            r#"{"current_topic":"t","cards":[
                {"kind":"decision","title":"Lançamento no dia 30","description":"a"},
                {"kind":"decision","title":"Lancamento no dia 30","description":"duplicata"}]}"#,
            10,
        )
        .unwrap();
        assert_eq!(
            res.cards.len(),
            1,
            "duplicata acentuada deveria ser mesclada"
        );
    }

    #[test]
    fn titulos_reformulados_contam_como_o_mesmo_card() {
        let a = card(
            CardKind::Decision,
            "Lançamento no dia 30",
            CardStatus::Proposed,
        );
        let b = card(
            CardKind::Decision,
            "Lancamento dia 30",
            CardStatus::Proposed,
        );
        assert!(is_same_card(&a, &b));

        // Tipo diferente nunca mescla, mesmo com título parecido.
        let c = card(
            CardKind::Action,
            "Lançamento no dia 30",
            CardStatus::Proposed,
        );
        assert!(!is_same_card(&a, &c));

        // Assunto claramente outro não mescla.
        let d = card(
            CardKind::Decision,
            "Contratar fornecedor novo",
            CardStatus::Proposed,
        );
        assert!(!is_same_card(&a, &d));
    }

    #[test]
    fn sanitiza_json_com_cercas_markdown() {
        let wrapped = "```json\n{\"current_topic\": \"Alinhamento\", \"cards\": []}\n```";
        let res = parse_analysis_json(wrapped, 60).unwrap();
        assert_eq!(res.current_topic, "Alinhamento");
        assert!(res.cards.is_empty());
    }

    #[test]
    fn sanitiza_json_embrulhado_em_texto() {
        let chatty = "Claro! Aqui está a análise:\n{\"current_topic\": \"Preço\", \"cards\": []}\nEspero ter ajudado.";
        let res = parse_analysis_json(chatty, 60).unwrap();
        assert_eq!(res.current_topic, "Preço");
    }

    #[test]
    fn json_invalido_vira_erro_e_nao_panico() {
        assert!(parse_analysis_json("desculpe, não consegui", 10).is_err());
    }

    #[test]
    fn gatilhos_locais_pegam_decisao_acao_e_risco() {
        assert_eq!(
            detect_trigger("Então fica combinado, entregamos dia 30."),
            Some(TriggerKind::Decision)
        );
        assert_eq!(
            detect_trigger("Eu envio a proposta revisada amanhã."),
            Some(TriggerKind::Action)
        );
        assert_eq!(
            detect_trigger("Discordo, isso me preocupa bastante."),
            Some(TriggerKind::Risk)
        );
        assert_eq!(detect_trigger("Bom dia pessoal, tudo certo?"), None);
        assert_eq!(detect_trigger("   "), None);
    }

    #[test]
    fn gatilho_funciona_sem_acento_e_com_caixa_alta() {
        // O Whisper às vezes devolve sem acento ou tudo em caixa alta.
        assert_eq!(
            detect_trigger("ENTAO FICA COMBINADO"),
            Some(TriggerKind::Decision)
        );
    }

    #[test]
    fn acao_sem_prazo_e_sinalizada() {
        let mut c = card(CardKind::Action, "Enviar relatório", CardStatus::Proposed);
        assert!(c.missing_due_date(), "sem due_date conta como pendente");

        c.due_date = Some("Sem prazo definido".into());
        assert!(c.missing_due_date());

        c.due_date = Some("Sexta 18h".into());
        assert!(!c.missing_due_date());

        // Só ação tem prazo a cobrar.
        let d = card(CardKind::Decision, "Fechar contrato", CardStatus::Proposed);
        assert!(!d.missing_due_date());
    }

    #[test]
    fn ata_so_traz_o_que_o_usuario_confirmou() {
        let cards = vec![
            card(CardKind::Decision, "Lançar dia 30", CardStatus::Confirmed),
            card(
                CardKind::Decision,
                "Trocar fornecedor",
                CardStatus::Proposed,
            ),
            card(
                CardKind::Risk,
                "SLA do fim de semana",
                CardStatus::Discarded,
            ),
        ];
        let md = render_decisions_markdown(&cards).expect("há uma confirmada");
        assert!(md.contains("Lançar dia 30"));
        assert!(
            !md.contains("Trocar fornecedor"),
            "proposto não entra na ata"
        );
        assert!(!md.contains("SLA do fim de semana"), "descartado não entra");
        assert!(md.contains("### Decisões"));
    }

    #[test]
    fn ata_e_none_quando_nada_foi_confirmado() {
        let cards = vec![card(CardKind::Decision, "X", CardStatus::Proposed)];
        assert!(render_decisions_markdown(&cards).is_none());
        assert!(render_decisions_markdown(&[]).is_none());
    }

    #[test]
    fn ata_cobra_prazo_em_aberto_e_mostra_responsavel() {
        let mut acao = card(CardKind::Action, "Enviar relatório", CardStatus::Confirmed);
        acao.owner = Some("Carlos".into());
        acao.due_date = Some("Sem prazo definido".into());
        let md = render_decisions_markdown(&[acao]).unwrap();
        assert!(md.contains("### Ações"));
        assert!(md.contains("responsável: Carlos"));
        assert!(md.contains("Prazo em aberto"));
    }

    #[test]
    fn prompt_lista_cards_anteriores_e_pede_para_nao_repetir() {
        let anteriores = vec![card(
            CardKind::Decision,
            "Lançar dia 30",
            CardStatus::Proposed,
        )];
        let p = build_analysis_prompt(&CopilotInput {
            window: "[00:10] Eu: bom dia",
            elapsed: "00:10",
            elapsed_secs: 10,
            previous_cards: &anteriores,
            focus: &[],
        });
        assert!(p.contains("Lançar dia 30"));
        assert!(p.contains("NÃO repita"));
        assert!(p.contains("00:10"));
        assert!(
            !p.contains("filtro rápido"),
            "sem foco, sem o bloco do filtro"
        );
    }

    #[test]
    fn rodada_focada_diz_onde_o_filtro_marcou() {
        let focus = [
            FocusHint {
                at_secs: 75.0,
                kind: CardKind::Action,
            },
            FocusHint {
                at_secs: 3725.0,
                kind: CardKind::Risk,
            },
        ];
        let p = build_analysis_prompt(&CopilotInput {
            window: "[01:15] Eu: eu te mando amanhã",
            elapsed: "1:02:10",
            elapsed_secs: 3730,
            previous_cards: &[],
            focus: &focus,
        });
        assert!(p.contains("filtro rápido"));
        assert!(p.contains("- [01:15] provável Ação"));
        assert!(p.contains("- [1:02:05] provável Alerta de Risco"));
        assert!(p.contains("não crie card"), "o filtro sugere, o LLM decide");
    }

    fn filter_decision(probs: [f32; 4]) -> crate::systemone::Decision {
        let labels = ["decision", "action", "risk", "none"];
        crate::systemone::Decision {
            model: "jev-1.13.0".into(),
            answers: vec![(
                FILTER_KIND.into(),
                crate::systemone::Answer::Choice {
                    chosen: String::new(),
                    probabilities: labels
                        .iter()
                        .zip(probs)
                        .map(|(l, p)| ((*l).to_string(), p))
                        .collect(),
                    confidence: 0.5,
                },
            )],
            input_tokens: 600,
        }
    }

    #[test]
    fn filtro_marca_pelo_complemento_de_nada_e_escolhe_o_tipo() {
        let v = read_filter(&filter_decision([0.1, 0.4, 0.2, 0.3])).unwrap();
        assert!((v.p_card - 0.7).abs() < 1e-6);
        assert_eq!(v.kind, CardKind::Action);
        assert!(v.flagged);

        let calmo = read_filter(&filter_decision([0.05, 0.05, 0.1, 0.8])).unwrap();
        assert!(!calmo.flagged, "p(card) 0,2 fica abaixo do limiar");
        assert_eq!(calmo.kind, CardKind::Risk);

        let limite = read_filter(&filter_decision([0.5, 0.0, 0.0, 0.5])).unwrap();
        assert!(limite.flagged, "o limiar é inclusivo");
    }

    #[test]
    fn filtro_sem_a_pergunta_nao_decide_nada() {
        let mut d = filter_decision([0.0, 0.0, 0.0, 1.0]);
        d.answers.clear();
        assert!(read_filter(&d).is_none());
    }

    #[test]
    fn pergunta_do_filtro_e_a_medida() {
        // A ordem e os rótulos foram os da medição (estudo, §12.5); a leitura
        // do veredito depende exatamente destes rótulos.
        let qs = filter_questions();
        assert_eq!(qs.len(), 1);
        let (id, q) = &qs[0];
        assert_eq!(*id, FILTER_KIND);
        match q {
            crate::systemone::Question::Choice { options, .. } => {
                let labels: Vec<&str> = options.iter().map(|(l, _)| l.as_str()).collect();
                assert_eq!(labels, ["decision", "action", "risk", "none"]);
            }
            other => panic!("esperava choice, veio {other:?}"),
        }
        let s = filter_state("Eu: oi", "Participantes: fica combinado");
        assert_eq!(s["atual"], "Participantes: fica combinado");
        assert_eq!(s["anterior"], "Eu: oi");
    }

    #[test]
    fn prompt_omite_cards_descartados_pelo_usuario() {
        // Descartar um card e ver a IA trazê-lo de volta seria irritante.
        let anteriores = vec![card(CardKind::Risk, "Ruído da sala", CardStatus::Discarded)];
        let p = build_analysis_prompt(&CopilotInput {
            window: "x",
            elapsed: "00:10",
            elapsed_secs: 10,
            previous_cards: &anteriores,
            focus: &[],
        });
        assert!(!p.contains("Ruído da sala"));
    }

    #[test]
    fn query_meeting_invoca_provider_corretamente() {
        let fake = FakeProvider::replying("- O valor citado foi R$ 50.000.");
        let ans = query_meeting(
            &fake,
            "[01:00] Participante: Nosso orçamento é 50 mil.",
            "Qual o orçamento?",
        )
        .unwrap();
        assert!(ans.contains("50.000"));
        let (sys, user) = fake.single_call();
        assert!(sys.contains("assistente executivo"));
        assert!(user.contains("Qual o orçamento?"));
    }

    #[test]
    fn analyze_meeting_propaga_erro_do_provider() {
        let fake = FakeProvider::failing(|| crate::LlmError::Http("status 429".into()));
        let err = analyze_meeting(
            &fake,
            &CopilotInput {
                window: "[00:10] Eu: oi",
                elapsed: "00:10",
                elapsed_secs: 10,
                previous_cards: &[],
                focus: &[],
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("429"));
    }

    #[test]
    fn query_meeting_stream_entrega_pedacos_e_o_texto_inteiro() {
        // O FakeProvider não implementa complete_stream, então cai na
        // implementação padrão do trait — é exatamente o que um provider sem
        // streaming faria, e a resposta precisa ser a mesma.
        let fake = FakeProvider::replying("  - foi R$ 50 mil.  ");
        let mut pedacos = Vec::new();
        let full = query_meeting_stream(&fake, "[01:00] P: 50 mil", "quanto?", &mut |t| {
            pedacos.push(t.to_string())
        })
        .unwrap();
        assert_eq!(full, "- foi R$ 50 mil.", "o texto final vem aparado");
        assert_eq!(pedacos.concat(), "  - foi R$ 50 mil.  ");
    }

    #[test]
    fn enrich_notes_stream_usa_o_mesmo_prompt_da_versao_simples() {
        let fake = FakeProvider::replying("- Deploy: sexta 14h");
        enrich_notes_stream(&fake, "[02:10] Eu: sexta às 14h.", "- deploy", &mut |_| {}).unwrap();
        let (sys, user) = fake.single_call();
        assert!(sys.contains("notas executivas"));
        assert!(user.contains("- deploy"));
        assert!(user.contains("sexta às 14h"));
    }

    #[test]
    fn erro_no_streaming_propaga_sem_texto_parcial() {
        let fake = FakeProvider::failing(|| crate::LlmError::Http("status 503".into()));
        let mut pedacos = Vec::new();
        let err = query_meeting_stream(&fake, "t", "q", &mut |t| pedacos.push(t.to_string()))
            .unwrap_err();
        assert!(err.to_string().contains("503"));
        assert!(
            pedacos.is_empty(),
            "nada deve ser entregue se a chamada falhou"
        );
    }

    #[test]
    fn enrich_notes_invoca_provider() {
        let fake = FakeProvider::replying("- Deploy: agendado para sexta 14h");
        let enriched =
            enrich_notes(&fake, "[02:10] Eu: Deploy na sexta às 14h.", "- deploy").unwrap();
        assert_eq!(enriched, "- Deploy: agendado para sexta 14h");
    }
}
