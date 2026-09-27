//! Filtro do Copilot: cada parágrafo da conversa passa pelo Jev antes do LLM.
//!
//! ## Por que um filtro na frente do LLM
//!
//! Sem ele, quem acha os cards é o pulso: a cada 45 s a IA relê até 20 min
//! de conversa. O gatilho local ([`isper_llm::detect_trigger`]) antecipa a
//! rodada, mas as frases fixas pegaram só 5% dos momentos numa amostra de 450
//! parágrafos de reuniões reais. O Jev lê cada parágrafo em ~0,4 s por uma
//! fração de centavo e, no mesmo teste, pegou 92% dos momentos mandando só 45%
//! dos parágrafos adiante (`docs/estudos/gemini-transcribe-e-jev.md`, §12.5).
//! Então o LLM passa a rodar logo depois de algo importante ser dito, olhando
//! só os minutos recentes com os trechos marcados em destaque
//! ([`FOCUS_WINDOW_SECS`]); o pulso completo vira rede de segurança
//! ([`FILTERED_PULSE_SECS`]).
//!
//! O Jev não cria card: a precisão dele a 0,5 é 0,49. Ele aponta, o LLM
//! confere e redige.
//!
//! ## O que é um parágrafo aqui
//!
//! A mesma regra da ata ([`isper_core::meeting::group_speech`]): falas do
//! mesmo falante, com pausa de até [`GROUP_GAP_SECS`] e até
//! [`GROUP_MAX_SECS`] de duração. Foi o parágrafo da ata que a medição usou.
//! Só passa o parágrafo **fechado**; o que ainda está sendo falado espera.
//!
//! ## Falhar aberto
//!
//! Se o Jev não responde (rede, 5xx), o trecho não se perde: a próxima rodada
//! focada lê a janela recente mesmo sem marca. Chave recusada, sem crédito ou
//! resposta que não se entende desligam o filtro **nesta reunião**, e o
//! Copilot volta ao pulso de sempre, com o motivo na tela.

use crate::prelude::*;
use isper_core::meeting::{GROUP_GAP_SECS, GROUP_MAX_SECS};
use isper_llm::{Classifier, FILTER_MIN_WORDS, FilterVerdict, FocusHint, LlmError};

/// Valores aceitos em `copilot_filter` (config.toml).
pub(crate) const FILTER_MODES: [&str; 2] = ["off", "jev"];

/// Quanto de conversa vai na rodada focada: o parágrafo marcado e o
/// suficiente antes dele para a IA entender o assunto.
pub(crate) const FOCUS_WINDOW_SECS: f32 = 3.0 * 60.0;

/// Com o filtro ligado, a releitura completa da conversa sai só a cada tanto:
/// ela cobre o que se espalha por vários parágrafos e as perguntas sugeridas,
/// que nenhum parágrafo sozinho denuncia.
pub(crate) const FILTERED_PULSE_SECS: u64 = 180;

/// Um parágrafo pronto para o filtro.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Paragraph {
    pub(crate) speaker: String,
    pub(crate) start_secs: f32,
    pub(crate) end_secs: f32,
    pub(crate) text: String,
}

impl Paragraph {
    /// `"Falante: texto"` — o formato medido.
    fn line(&self) -> String {
        format!("{}: {}", self.speaker, self.text)
    }
}

/// Os parágrafos fechados da conversa ao vivo que terminam depois de
/// `from_secs`.
///
/// Fechado é o parágrafo seguido de outra fala que não cabe nele, ou cuja
/// última fala acabou há mais que a pausa ([`GROUP_GAP_SECS`]) no relógio da
/// reunião. As falas dos dois canais chegam na ordem em que a transcrição
/// termina, não na em que foram ditas — por isso a ordenação.
pub(crate) fn closed_paragraphs(
    live: &[LiveSegment],
    from_secs: f32,
    now_secs: f32,
) -> Vec<Paragraph> {
    let mut segs: Vec<&LiveSegment> = live
        .iter()
        .filter(|s| !s.provisional && s.end_secs > from_secs && !s.text.trim().is_empty())
        .collect();
    segs.sort_by(|a, b| a.start_secs.total_cmp(&b.start_secs));

    let mut out = Vec::new();
    let mut cur: Option<Paragraph> = None;
    for s in segs {
        match cur.as_mut() {
            Some(p)
                if p.speaker == s.speaker
                    && s.start_secs - p.end_secs <= GROUP_GAP_SECS
                    && s.end_secs - p.start_secs <= GROUP_MAX_SECS =>
            {
                p.end_secs = p.end_secs.max(s.end_secs);
                p.text.push(' ');
                p.text.push_str(s.text.trim());
            }
            _ => {
                out.extend(cur.take());
                cur = Some(Paragraph {
                    speaker: s.speaker.clone(),
                    start_secs: s.start_secs,
                    end_secs: s.end_secs,
                    text: s.text.trim().to_string(),
                });
            }
        }
    }
    if let Some(p) = cur
        && now_secs - p.end_secs > GROUP_GAP_SECS
    {
        out.push(p);
    }
    out
}

/// O que aconteceu com um parágrafo.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Outcome {
    /// Curto demais para virar card: nem foi ao Jev.
    Skipped,
    Read {
        verdict: FilterVerdict,
        tokens: u32,
    },
    /// Sem resposta (rede, 5xx): o trecho vai ao LLM do mesmo jeito.
    Failed(String),
    /// Não adianta insistir nesta reunião (chave, crédito, formato).
    Fatal(String),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Read {
    pub(crate) para: Paragraph,
    pub(crate) outcome: Outcome,
}

/// Passa os parágrafos pelo classificador, em ordem, cada um com o anterior
/// como contexto. Para no primeiro erro fatal: os seguintes ficam sem ler.
///
/// Roda sem nenhum lock do app — são chamadas de rede.
pub(crate) fn classify_paragraphs(
    previous: &str,
    paragraphs: Vec<Paragraph>,
    classifier: &dyn Classifier,
) -> Vec<Read> {
    let questions = isper_llm::filter_questions();
    let mut prev = previous.to_string();
    let mut out = Vec::with_capacity(paragraphs.len());
    for para in paragraphs {
        let line = para.line();
        let outcome = if para.text.split_whitespace().count() < FILTER_MIN_WORDS {
            Outcome::Skipped
        } else {
            match classifier.classify(&isper_llm::filter_state(&prev, &line), &questions) {
                Ok(d) => match isper_llm::read_filter(&d) {
                    Some(verdict) => Outcome::Read {
                        verdict,
                        tokens: d.input_tokens,
                    },
                    None => Outcome::Fatal("o Jev respondeu sem a pergunta do filtro".into()),
                },
                Err(
                    e @ (LlmError::Refused(_) | LlmError::NoApiKey(_) | LlmError::BadResponse(_)),
                ) => Outcome::Fatal(e.to_string()),
                Err(e) => Outcome::Failed(e.to_string()),
            }
        };
        let fatal = matches!(outcome, Outcome::Fatal(_));
        prev = line;
        out.push(Read { para, outcome });
        if fatal {
            break;
        }
    }
    out
}

/// O filtro dentro do estado do Copilot de uma reunião.
#[derive(Debug, Default, Clone)]
pub(crate) struct FilterState {
    /// Esta reunião começou com o filtro ligado e com chave.
    pub(crate) on: bool,
    /// Até onde (fim, no relógio da reunião) a conversa já passou pelo filtro.
    pub(crate) read_upto_secs: f32,
    /// O último parágrafo lido, contexto do próximo.
    pub(crate) previous: String,
    /// Trechos marcados que a próxima rodada focada ainda não viu.
    pub(crate) pending: Vec<FocusHint>,
    /// Um trecho ficou sem resposta: a próxima rodada focada lê a janela
    /// recente mesmo sem marca.
    pub(crate) missed: bool,
    pub(crate) paragraphs: u32,
    pub(crate) flagged: u32,
    pub(crate) failures: u32,
    pub(crate) input_tokens: u64,
    /// Por que o filtro caiu nesta reunião (a tela mostra).
    pub(crate) disabled: Option<String>,
}

impl FilterState {
    /// Ligado e funcionando.
    pub(crate) fn active(&self) -> bool {
        self.on && self.disabled.is_none()
    }

    pub(crate) fn apply(&mut self, reads: Vec<Read>) {
        for r in reads {
            self.read_upto_secs = self.read_upto_secs.max(r.para.end_secs);
            self.previous = r.para.line();
            match r.outcome {
                Outcome::Skipped => {}
                Outcome::Read { verdict, tokens } => {
                    self.paragraphs += 1;
                    self.input_tokens += u64::from(tokens);
                    if verdict.flagged {
                        self.flagged += 1;
                        self.pending.push(FocusHint {
                            at_secs: r.para.start_secs,
                            kind: verdict.kind,
                        });
                    }
                }
                Outcome::Failed(e) => {
                    self.failures += 1;
                    self.missed = true;
                    tracing::warn!("copilot: o filtro não respondeu ({e}); o trecho vai à IA");
                }
                Outcome::Fatal(e) => {
                    tracing::warn!("copilot: filtro desligado nesta reunião: {e}");
                    self.disabled = Some(e);
                }
            }
        }
    }

    /// Há o que mandar para uma rodada focada.
    pub(crate) fn wants_round(&self) -> bool {
        !self.pending.is_empty() || self.missed
    }

    /// Os trechos da próxima rodada focada (e o pedido de rodada some).
    pub(crate) fn take_focus(&mut self) -> Vec<FocusHint> {
        self.missed = false;
        std::mem::take(&mut self.pending)
    }

    /// A janela do Copilot abriu (ou reabriu): a rodada completa já lê o que
    /// passou, então o filtro recomeça do agora — ler o atrasado custaria
    /// chamadas para cards que a rodada completa já vai trazer.
    pub(crate) fn skip_to(&mut self, secs: f32) {
        self.read_upto_secs = self.read_upto_secs.max(secs);
        self.pending.clear();
        self.missed = false;
    }

    pub(crate) fn cost_usd(&self) -> f64 {
        self.input_tokens as f64 * isper_llm::systemone::JEV_USD_PER_MTOK / 1e6
    }

    pub(crate) fn dto(&self) -> Option<FilterDto> {
        (self.on || self.disabled.is_some()).then(|| FilterDto {
            paragraphs: self.paragraphs,
            flagged: self.flagged,
            failures: self.failures,
            cost_usd: self.cost_usd(),
            disabled: self.disabled.clone(),
        })
    }
}

/// O filtro na tela do Copilot.
#[derive(Clone, serde::Serialize)]
pub(crate) struct FilterDto {
    pub(crate) paragraphs: u32,
    pub(crate) flagged: u32,
    pub(crate) failures: u32,
    pub(crate) cost_usd: f64,
    pub(crate) disabled: Option<String>,
}

/// O classificador da reunião que começa: `Ok(None)` com o filtro desligado;
/// `Err` com ele ligado e sem chave (o motivo vai para a tela).
pub(crate) fn classifier_for(mode: &str) -> Result<Option<Box<dyn Classifier>>, String> {
    if mode != "jev" {
        return Ok(None);
    }
    isper_llm::Jev::from_keyring()
        .map(|j| Some(Box::new(j) as Box<dyn Classifier>))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use isper_llm::CardKind;
    use isper_llm::testing::{FakeClassifier, filter_decision};

    fn seg(speaker: &str, start: f32, end: f32, text: &str) -> LiveSegment {
        LiveSegment {
            speaker: speaker.into(),
            start_secs: start,
            end_secs: end,
            text: text.into(),
            provisional: false,
        }
    }

    const LONGO: &str = "então fica combinado que eu mando a proposta revisada amanhã cedo";

    #[test]
    fn junta_falas_do_mesmo_falante_e_fecha_na_troca() {
        let live = [
            seg("Eu", 0.0, 5.0, "primeira parte"),
            seg("Eu", 6.0, 10.0, "segunda parte"),
            seg("Participantes", 11.0, 15.0, "resposta deles"),
        ];
        let ps = closed_paragraphs(&live, 0.0, 16.0);
        assert_eq!(ps.len(), 1, "o do outro falante ainda está aberto");
        assert_eq!(ps[0].text, "primeira parte segunda parte");
        assert_eq!((ps[0].start_secs, ps[0].end_secs), (0.0, 10.0));

        let ps = closed_paragraphs(&live, 0.0, 20.0);
        assert_eq!(ps.len(), 2, "passou a pausa: o último fecha");
        assert_eq!(ps[1].speaker, "Participantes");
    }

    #[test]
    fn pausa_longa_e_limite_de_duracao_quebram_o_paragrafo() {
        let live = [
            seg("Eu", 0.0, 5.0, "a"),
            seg("Eu", 10.0, 15.0, "b"), // pausa de 5 s > 4 s
            seg("Eu", 16.0, 50.0, "c"),
            seg("Eu", 51.0, 80.0, "d"), // passaria de 60 s desde 10 s
        ];
        let ps = closed_paragraphs(&live, 0.0, 100.0);
        let textos: Vec<&str> = ps.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(textos, ["a", "b c", "d"]);
    }

    #[test]
    fn ignora_provisorio_o_ja_lido_e_ordena_os_canais() {
        let mut prov = seg("Eu", 30.0, 35.0, "rascunho");
        prov.provisional = true;
        let live = [
            seg("Participantes", 12.0, 16.0, "chegou depois na fila"),
            seg("Eu", 0.0, 10.0, "já lido"),
            seg("Eu", 20.0, 25.0, "novo"),
            prov,
        ];
        let ps = closed_paragraphs(&live, 10.0, 40.0);
        let textos: Vec<&str> = ps.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(textos, ["chegou depois na fila", "novo"]);
    }

    fn para(start: f32, text: &str) -> Paragraph {
        Paragraph {
            speaker: "Participantes".into(),
            start_secs: start,
            end_secs: start + 10.0,
            text: text.into(),
        }
    }

    #[test]
    fn classifica_com_o_anterior_como_contexto_e_pula_o_curto() {
        let fake = FakeClassifier::card_probability(0.9, "action");
        let reads = classify_paragraphs(
            "Eu: bom dia",
            vec![para(0.0, "uhum, beleza"), para(10.0, LONGO)],
            &fake,
        );
        assert_eq!(reads[0].outcome, Outcome::Skipped);
        assert!(
            matches!(reads[1].outcome, Outcome::Read { verdict, tokens: 600 } if verdict.flagged)
        );
        let calls = fake.calls();
        assert_eq!(calls.len(), 1, "o curto nem foi ao Jev");
        assert_eq!(calls[0]["anterior"], "Participantes: uhum, beleza");
        assert_eq!(calls[0]["atual"], format!("Participantes: {LONGO}"));
    }

    #[test]
    fn erro_fatal_para_a_leitura_e_desliga_o_filtro() {
        let fake = FakeClassifier::failing(|| LlmError::Refused("sem créditos (402)".into()));
        let reads = classify_paragraphs("", vec![para(0.0, LONGO), para(10.0, LONGO)], &fake);
        assert_eq!(reads.len(), 1, "não insiste depois de um erro fatal");
        let mut f = FilterState {
            on: true,
            ..Default::default()
        };
        f.apply(reads);
        assert!(!f.active());
        assert!(f.dto().unwrap().disabled.unwrap().contains("402"));
    }

    #[test]
    fn falha_de_rede_nao_perde_o_trecho() {
        let fake = FakeClassifier::failing(|| LlmError::Http("timeout".into()));
        let mut f = FilterState {
            on: true,
            ..Default::default()
        };
        f.apply(classify_paragraphs("", vec![para(0.0, LONGO)], &fake));
        assert!(f.active(), "rede instável não desliga o filtro");
        assert_eq!(f.failures, 1);
        assert!(f.wants_round(), "a rodada focada sai mesmo sem marca");
        assert!(f.take_focus().is_empty());
        assert!(!f.wants_round());
    }

    #[test]
    fn marca_so_o_que_passa_do_limiar_e_soma_o_custo() {
        let fake = FakeClassifier::with(|state| {
            let atual = state["atual"].as_str().unwrap_or_default();
            Ok(if atual.contains("combinado") {
                filter_decision(0.8, "decision")
            } else {
                filter_decision(0.2, "risk")
            })
        });
        let mut f = FilterState {
            on: true,
            ..Default::default()
        };
        f.apply(classify_paragraphs(
            "",
            vec![
                para(
                    0.0,
                    "a gente estava só explicando como funciona a tela de pedidos hoje",
                ),
                para(12.0, LONGO),
            ],
            &fake,
        ));
        assert_eq!((f.paragraphs, f.flagged), (2, 1));
        assert_eq!(f.read_upto_secs, 22.0);
        assert!(f.previous.contains("combinado"));
        let focus = f.take_focus();
        assert_eq!(focus.len(), 1);
        assert_eq!(focus[0].kind, CardKind::Decision);
        assert_eq!(focus[0].at_secs, 12.0);
        assert!((f.cost_usd() - 1200.0 * 0.042e-6).abs() < 1e-12);
    }

    #[test]
    fn abrir_a_janela_pula_o_atrasado() {
        let mut f = FilterState {
            on: true,
            read_upto_secs: 30.0,
            missed: true,
            ..Default::default()
        };
        f.pending.push(FocusHint {
            at_secs: 10.0,
            kind: CardKind::Risk,
        });
        f.skip_to(600.0);
        assert_eq!(f.read_upto_secs, 600.0);
        assert!(!f.wants_round());
    }

    #[test]
    fn sem_filtro_nao_ha_classificador_nem_quadro() {
        assert!(matches!(classifier_for("off"), Ok(None)));
        assert!(FilterState::default().dto().is_none());
    }
}
