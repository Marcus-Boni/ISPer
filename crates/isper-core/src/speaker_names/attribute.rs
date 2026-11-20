//! Quem disse cada fala do canal dos participantes, cruzando o que o Teams
//! mostrou durante a chamada com o tempo das falas do passe final.
//!
//! Nunca é um palpite. Uma fala ganha um nome só quando as três camadas
//! concordam:
//!
//! 1. **No trecho**: durante mais da metade da fala, a leitura do Teams
//!    apontava uma pessoa só — marcada falando e presente uma única vez no
//!    painel Pessoas, ou a única outra pessoa numa chamada de dois.
//! 2. **No grupo da diarização**: entre as falas do mesmo "Participante N"
//!    com evidência, pelo menos [`Rules::min_group_segments`] apontam a mesma
//!    pessoa, e ela tem [`Rules::group_consensus`] da duração com evidência.
//!    Um grupo que aponta pessoas diferentes fica sem nome.
//! 3. **Entre grupos**: uma pessoa que dois grupos reivindicam não fica com
//!    nenhum deles.
//!
//! E o nome vai só para as falas do grupo que também têm a evidência no
//! próprio trecho. "Eu" (o microfone) nunca entra aqui; "Participantes"
//! (sem diarização) também não.
//!
//! Tudo é determinístico: as mesmas leituras e falas dão sempre os mesmos
//! nomes, em qualquer ordem que as leituras cheguem.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::teams::{CallReading, name_key};

/// Em que estado estava a leitura.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ObservationState {
    /// A janela de chamada do Teams cujo áudio está sendo gravado foi lida.
    Ok,
    /// O Teams está aberto, mas sem janela de chamada à vista.
    NoCallWindow,
    /// O Teams não está rodando.
    NotRunning,
    /// Mais de uma janela de chamada com informação: não dá para saber qual
    /// é a gravada.
    Ambiguous,
    /// O áudio gravado deixou de ser o deste Teams (caiu para o sistema, o
    /// processo reiniciou) ou o leitor recomeçou: a evidência fica suspensa
    /// até a próxima leitura boa.
    Unbound,
    /// O UI Automation falhou nesta leitura.
    Error,
}

/// Uma leitura do Teams, no relógio da reunião.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    /// Quando a tela foi lida, em segundos desde o início da gravação.
    pub at_secs: f64,
    /// Se a leitura vale.
    pub state: ObservationState,
    /// O que a janela mostrava (vazio fora de [`ObservationState::Ok`]).
    #[serde(default)]
    pub reading: CallReading,
}

impl Observation {
    /// Uma leitura que não vale (para marcar uma lacuna).
    pub fn gap(at_secs: f64, state: ObservationState) -> Self {
        Self {
            at_secs,
            state,
            reading: CallReading::default(),
        }
    }
}

/// Os limiares da atribuição. Os padrões são conservadores; a calibração
/// com chamadas marcadas (plano, seção 1) é que os ajusta.
#[derive(Debug, Clone, PartialEq)]
pub struct Rules {
    /// Quanto o Teams demora para mostrar quem começou a falar. A leitura
    /// das `t` segundos fala do áudio de `t - ui_lag_secs`.
    pub ui_lag_secs: f64,
    /// Uma leitura vale até a próxima, mas nunca por mais que isto: um
    /// leitor parado não continua dando nome a ninguém.
    pub hold_secs: f64,
    /// A pessoa precisa cobrir mais que esta fração da fala.
    pub segment_majority: f64,
    /// Falas do grupo que precisam apontar a mesma pessoa.
    pub min_group_segments: usize,
    /// Fração da duração com evidência do grupo que a pessoa precisa ter.
    pub group_consensus: f64,
    /// Exigir o painel Pessoas para validar quem foi marcado falando (sem
    /// ele, não há como saber se há homônimos).
    pub require_roster: bool,
    /// Numa chamada de duas pessoas, com "(Você)" conhecido, o áudio dos
    /// participantes é o da outra pessoa.
    pub two_party: bool,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            ui_lag_secs: 0.75,
            hold_secs: 2.0,
            segment_majority: 0.5,
            min_group_segments: 2,
            group_consensus: 0.8,
            require_roster: true,
            two_party: true,
        }
    }
}

/// Uma fala do canal dos participantes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RemoteSegment {
    /// O grupo da diarização ("Participante N" → `N`); `None` sem
    /// diarização, e a fala não recebe nome.
    pub group: Option<u32>,
    /// Início, em segundos da reunião.
    pub start_secs: f64,
    /// Fim, em segundos da reunião.
    pub end_secs: f64,
}

/// O que a atribuição fez, em números — vai para o log (nunca nomes).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct AttributionReport {
    /// Leituras recebidas.
    pub observations: usize,
    /// Leituras que apontavam uma pessoa.
    pub decisive_observations: usize,
    /// Falas com grupo de diarização.
    pub segments: usize,
    /// Falas sem grupo (rótulo "Participantes").
    pub ungrouped_segments: usize,
    /// Falas cuja evidência no próprio trecho apontava uma pessoa.
    pub segments_with_evidence: usize,
    /// Falas que ficaram com nome.
    pub named_segments: usize,
    /// Grupos de diarização vistos.
    pub groups: usize,
    /// Grupos associados a uma pessoa.
    pub named_groups: usize,
    /// Grupos cuja evidência apontava pessoas diferentes.
    pub split_groups: usize,
    /// Grupos com evidência de menos.
    pub thin_groups: usize,
    /// Pessoas reivindicadas por mais de um grupo (ficaram sem nome).
    pub disputed_names: usize,
    /// Duração das falas com grupo, em segundos.
    pub remote_secs: f64,
    /// Duração das falas que ficaram com nome.
    pub named_secs: f64,
}

/// O resultado: um nome (ou nada) por fala, na ordem recebida.
#[derive(Debug, Clone, PartialEq)]
pub struct Attribution {
    /// `names[i]` é o nome da fala `i`, quando há.
    pub names: Vec<Option<String>>,
    /// Os números do que aconteceu.
    pub report: AttributionReport,
}

/// O grupo de um rótulo do passe final: "Participante 3" → 3. "Eu",
/// "Participantes" e nomes dados à mão não têm grupo.
pub fn group_of(label: &str) -> Option<u32> {
    label
        .strip_prefix("Participante ")
        .and_then(|n| n.parse().ok())
}

/// Quem a leitura aponta como a voz do canal dos participantes agora.
fn who_at(o: &Observation, rules: &Rules) -> Option<(String, String)> {
    if o.state != ObservationState::Ok {
        return None;
    }
    let r = &o.reading;
    let me = r.me.as_deref().map(name_key);
    let is_me = |n: &str| me.as_deref() == Some(name_key(n).as_str());
    // Presença: quantas linhas do painel cada nome tem (2+ = homônimos).
    let mut roster: BTreeMap<String, (usize, &str)> = BTreeMap::new();
    for p in &r.participants {
        roster.entry(name_key(p)).or_insert((0, p)).0 += 1;
    }
    let mut speakers: BTreeMap<String, &str> = BTreeMap::new();
    for s in r.speaking.iter().filter(|s| !is_me(s)) {
        speakers.entry(name_key(s)).or_insert(s);
    }
    match speakers.len() {
        // Duas pessoas falando ao mesmo tempo: a fala é de quem?
        2.. => None,
        1 => {
            let (key, shown) = speakers.into_iter().next()?;
            match roster.get(&key) {
                Some((1, _)) => Some((key, shown.to_string())),
                // Homônimos: não há como saber qual dos dois.
                Some(_) => None,
                None if roster.is_empty() && !rules.require_roster => {
                    Some((key, shown.to_string()))
                }
                // Marcado, mas fora do painel: não validado.
                None => None,
            }
        }
        0 => {
            if !rules.two_party || roster.len() != 2 || roster.values().any(|(n, _)| *n != 1) {
                return None;
            }
            let me = me?;
            if !roster.contains_key(&me) {
                return None;
            }
            roster
                .into_iter()
                .find(|(k, _)| *k != me)
                .map(|(k, (_, shown))| (k, shown.to_string()))
        }
    }
}

/// Um trecho do relógio do áudio em que uma leitura vale.
struct Span {
    start: f64,
    end: f64,
    who: Option<(String, String)>,
}

/// As leituras como trechos do relógio do ÁUDIO: cada uma vale da hora em
/// que foi feita até a próxima (no máximo [`Rules::hold_secs`]), recuada de
/// [`Rules::ui_lag_secs`].
fn spans(observations: &[Observation], rules: &Rules) -> Vec<Span> {
    let mut obs: Vec<&Observation> = observations
        .iter()
        .filter(|o| o.at_secs.is_finite())
        .collect();
    // Estável: duas leituras no mesmo instante mantêm a ordem de chegada,
    // e a de estado pior não some atrás da boa.
    obs.sort_by(|a, b| a.at_secs.total_cmp(&b.at_secs));
    obs.iter()
        .enumerate()
        .map(|(i, o)| {
            let next = obs.get(i + 1).map_or(f64::INFINITY, |n| n.at_secs);
            Span {
                start: o.at_secs - rules.ui_lag_secs,
                end: next.min(o.at_secs + rules.hold_secs) - rules.ui_lag_secs,
                who: who_at(o, rules),
            }
        })
        .collect()
}

/// A pessoa que cobre mais da metade da fala, pela evidência do trecho.
fn segment_evidence(
    seg: &RemoteSegment,
    spans: &[Span],
    rules: &Rules,
) -> Option<(String, String)> {
    let (s, e) = (seg.start_secs, seg.end_secs);
    // `end` cresce com `start` (cada trecho termina antes do próximo
    // começar): a busca binária acha o primeiro que pode tocar a fala.
    let first = spans.partition_point(|sp| sp.end <= s);
    if e <= s {
        // Fala sem duração: quem valia no instante em que ela começou.
        return spans
            .get(first)
            .filter(|sp| sp.start <= s && s < sp.end)
            .and_then(|sp| sp.who.clone());
    }
    let mut time: BTreeMap<&str, (f64, &str)> = BTreeMap::new();
    for sp in spans[first..].iter().take_while(|sp| sp.start < e) {
        let overlap = sp.end.min(e) - sp.start.max(s);
        if overlap > 0.0
            && let Some((key, shown)) = &sp.who
        {
            time.entry(key).or_insert((0.0, shown)).0 += overlap;
        }
    }
    let (key, (secs, shown)) = time.into_iter().max_by(|a, b| a.1.0.total_cmp(&b.1.0))?;
    (secs > rules.segment_majority * (e - s)).then(|| (key.to_string(), shown.to_string()))
}

/// Dá nome às falas do canal dos participantes (ver o topo do módulo).
pub fn attribute(
    observations: &[Observation],
    segments: &[RemoteSegment],
    rules: &Rules,
) -> Attribution {
    let spans = spans(observations, rules);
    let mut report = AttributionReport {
        observations: observations.len(),
        decisive_observations: spans.iter().filter(|s| s.who.is_some()).count(),
        ..AttributionReport::default()
    };

    // 1. A evidência de cada fala no próprio trecho.
    let evidence: Vec<Option<(String, String)>> = segments
        .iter()
        .map(|seg| {
            if seg.group.is_none() {
                report.ungrouped_segments += 1;
                return None;
            }
            report.segments += 1;
            report.remote_secs += (seg.end_secs - seg.start_secs).max(0.0);
            let ev = segment_evidence(seg, &spans, rules);
            report.segments_with_evidence += usize::from(ev.is_some());
            ev
        })
        .collect();

    // 2. O consenso dentro de cada grupo da diarização.
    #[derive(Default)]
    struct Tally<'a> {
        by_name: BTreeMap<&'a str, (f64, usize)>,
        secs: f64,
    }
    let mut groups: BTreeMap<u32, Tally<'_>> = BTreeMap::new();
    for (seg, ev) in segments.iter().zip(&evidence) {
        let Some(g) = seg.group else { continue };
        let tally = groups.entry(g).or_default();
        if let Some((key, _)) = ev {
            let dur = (seg.end_secs - seg.start_secs).max(0.0);
            let entry = tally.by_name.entry(key).or_insert((0.0, 0));
            entry.0 += dur;
            entry.1 += 1;
            tally.secs += dur;
        }
    }
    report.groups = groups.len();
    let mut chosen: BTreeMap<u32, &str> = BTreeMap::new();
    // Quem cada grupo reivindica: a pessoa com a maior parte da evidência
    // dele (metade ou mais), MESMO que o grupo não tenha evidência bastante
    // para ganhar o nome. Um grupo de uma fala só que aponta a Ana ainda é
    // uma voz diferente dizendo "sou a Ana".
    let mut claims: BTreeMap<&str, usize> = BTreeMap::new();
    for (g, tally) in &groups {
        let mut ranked: Vec<(&str, f64, usize)> = tally
            .by_name
            .iter()
            .map(|(k, (secs, n))| (*k, *secs, *n))
            .collect();
        // Estável sobre a ordem alfabética das chaves: empate é empate.
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        let Some(&(key, secs, count)) = ranked.first() else {
            report.thin_groups += 1;
            continue;
        };
        let tie = ranked.get(1).is_some_and(|r| r.1 >= secs);
        if !tie && secs >= 0.5 * tally.secs {
            *claims.entry(key).or_default() += 1;
        }
        if tie || secs < rules.group_consensus * tally.secs {
            report.split_groups += 1;
        } else if count < rules.min_group_segments {
            report.thin_groups += 1;
        } else {
            chosen.insert(*g, key);
        }
    }

    // 3. Uma pessoa, um grupo: a reivindicada por dois não fica com nenhum.
    report.disputed_names = claims.values().filter(|n| **n > 1).count();
    chosen.retain(|_, key| claims.get(key).copied().unwrap_or(0) == 1);
    report.named_groups = chosen.len();

    // A grafia de cada pessoa: a da primeira leitura em que ela apareceu,
    // para todas as falas dela saírem iguais.
    let mut shown_as: BTreeMap<&str, &str> = BTreeMap::new();
    for (key, shown) in spans.iter().filter_map(|s| s.who.as_ref()) {
        shown_as.entry(key).or_insert(shown);
    }

    // O nome vai só para as falas do grupo com a evidência no trecho.
    let names: Vec<Option<String>> = segments
        .iter()
        .zip(&evidence)
        .map(|(seg, ev)| {
            let (key, shown) = ev.as_ref()?;
            let g = seg.group?;
            (chosen.get(&g) == Some(&key.as_str())).then(|| {
                report.named_segments += 1;
                report.named_secs += (seg.end_secs - seg.start_secs).max(0.0);
                shown_as
                    .get(key.as_str())
                    .copied()
                    .unwrap_or(shown)
                    .to_string()
            })
        })
        .collect();
    Attribution { names, report }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: &str = "Bruno Lima";

    fn reading(speaking: &[&str], participants: &[&str], me: Option<&str>) -> CallReading {
        CallReading {
            speaking: speaking.iter().map(|s| s.to_string()).collect(),
            participants: participants.iter().map(|s| s.to_string()).collect(),
            me: me.map(Into::into),
        }
    }

    /// Leituras a cada 0,5 s de `from` a `to` (relógio da tela), todas iguais.
    fn every(from: f64, to: f64, r: &CallReading) -> Vec<Observation> {
        let mut out = Vec::new();
        let mut t = from;
        while t < to {
            out.push(Observation {
                at_secs: t,
                state: ObservationState::Ok,
                reading: r.clone(),
            });
            t += 0.5;
        }
        out
    }

    fn seg(group: u32, start: f64, end: f64) -> RemoteSegment {
        RemoteSegment {
            group: Some(group),
            start_secs: start,
            end_secs: end,
        }
    }

    /// Regras sem atraso da tela, para os tempos dos testes serem os da fala.
    fn no_lag() -> Rules {
        Rules {
            ui_lag_secs: 0.0,
            ..Rules::default()
        }
    }

    fn grupo3(speaking: &[&str]) -> CallReading {
        reading(speaking, &[ME, "Ana Souza", "João Ávila"], Some(ME))
    }

    #[test]
    fn marca_explicita_da_nome_ao_grupo_inteiro() {
        let mut obs = every(0.0, 10.0, &grupo3(&["Ana Souza"]));
        obs.extend(every(10.0, 20.0, &grupo3(&["João Ávila"])));
        let segs = [
            seg(1, 1.0, 4.0),
            seg(2, 11.0, 14.0),
            seg(1, 5.0, 9.0),
            seg(2, 15.0, 19.0),
        ];
        let a = attribute(&obs, &segs, &no_lag());
        assert_eq!(
            a.names,
            [
                Some("Ana Souza".into()),
                Some("João Ávila".into()),
                Some("Ana Souza".into()),
                Some("João Ávila".into())
            ]
        );
        assert_eq!(a.report.named_groups, 2);
        assert_eq!(a.report.named_segments, 4);
        assert!((a.report.named_secs - 14.0).abs() < 1e-9);
    }

    #[test]
    fn o_atraso_da_tela_e_descontado() {
        // A tela marca a Ana 0,75 s depois de ela começar.
        let obs = every(0.75, 10.75, &grupo3(&["Ana Souza"]));
        let segs = [seg(1, 0.0, 3.0), seg(1, 6.0, 10.0)];
        assert_eq!(
            attribute(&obs, &segs, &Rules::default()).names,
            [Some("Ana Souza".into()), Some("Ana Souza".into())]
        );
        // O primeiro segundo da fala só é coberto com o desconto: sem ele, a
        // tela só "chega" aos 0,75 s, e a fala de 0 a 1 s fica sem maioria.
        let segs = [seg(1, 0.0, 1.0), seg(1, 5.0, 8.0)];
        assert_eq!(
            attribute(&obs, &segs, &Rules::default()).names[0],
            Some("Ana Souza".into())
        );
        assert_eq!(attribute(&obs, &segs, &no_lag()).names[0], None);
    }

    #[test]
    fn leitura_parada_nao_segura_o_nome_para_sempre() {
        // Uma leitura só, no segundo 0: vale até 2 s.
        let obs = every(0.0, 0.5, &grupo3(&["Ana Souza"]));
        let segs = [seg(1, 0.0, 1.5), seg(1, 0.2, 1.8), seg(1, 3.0, 6.0)];
        let a = attribute(&obs, &segs, &no_lag());
        assert_eq!(a.names[2], None);
        assert_eq!(a.names[0], Some("Ana Souza".into()));
    }

    #[test]
    fn duas_pessoas_falando_ao_mesmo_tempo_ficam_genericas() {
        let obs = every(0.0, 10.0, &grupo3(&["Ana Souza", "João Ávila"]));
        let a = attribute(&obs, &[seg(1, 1.0, 4.0), seg(1, 5.0, 9.0)], &no_lag());
        assert_eq!(a.names, [None, None]);
        assert_eq!(a.report.decisive_observations, 0);
    }

    #[test]
    fn grupo_que_aponta_pessoas_diferentes_fica_sem_nome() {
        let mut obs = every(0.0, 10.0, &grupo3(&["Ana Souza"]));
        obs.extend(every(10.0, 20.0, &grupo3(&["João Ávila"])));
        // O mesmo "Participante 1" nos dois trechos.
        let a = attribute(
            &obs,
            &[seg(1, 1.0, 4.0), seg(1, 5.0, 9.0), seg(1, 11.0, 18.0)],
            &no_lag(),
        );
        assert_eq!(a.names, [None, None, None]);
        assert_eq!(a.report.split_groups, 1);
    }

    #[test]
    fn dois_grupos_com_a_mesma_pessoa_perdem_os_dois() {
        let obs = every(0.0, 20.0, &grupo3(&["Ana Souza"]));
        let segs = [
            seg(1, 1.0, 4.0),
            seg(1, 5.0, 8.0),
            seg(2, 11.0, 14.0),
            seg(2, 15.0, 18.0),
        ];
        let a = attribute(&obs, &segs, &no_lag());
        assert_eq!(a.names, [None, None, None, None]);
        assert_eq!(a.report.disputed_names, 1);
        assert_eq!(a.report.named_groups, 0);
    }

    #[test]
    fn grupo_de_uma_fala_so_tambem_disputa_a_pessoa() {
        // Visto no e2e: duas vozes, a tela marcando a mesma pessoa o tempo
        // todo. O grupo 2 tem uma fala só (pouca evidência para ganhar o
        // nome), mas é outra voz dizendo "sou a Ana": o grupo 1 não fica
        // com ela.
        let obs = every(0.0, 20.0, &grupo3(&["Ana Souza"]));
        let segs = [seg(1, 1.0, 4.0), seg(1, 5.0, 8.0), seg(2, 11.0, 14.0)];
        let a = attribute(&obs, &segs, &no_lag());
        assert_eq!(a.names, [None, None, None]);
        assert_eq!(a.report.disputed_names, 1);
        assert_eq!(a.report.thin_groups, 1);
        // Um grupo que aponta OUTRA pessoa em boa parte não disputa a Ana
        // por causa de uma fala de borda.
        let mut obs = every(0.0, 10.0, &grupo3(&["Ana Souza"]));
        obs.extend(every(10.0, 30.0, &grupo3(&["João Ávila"])));
        let segs = [
            seg(1, 1.0, 4.0),
            seg(1, 5.0, 8.0),
            seg(2, 8.5, 10.0),
            seg(2, 12.0, 18.0),
            seg(2, 20.0, 26.0),
        ];
        let a = attribute(&obs, &segs, &no_lag());
        assert_eq!(a.names[0], Some("Ana Souza".into()), "{:?}", a.report);
        assert_eq!(a.names[3], Some("João Ávila".into()));
        assert_eq!(
            a.names[2], None,
            "a fala de borda não ganha o nome do grupo"
        );
    }

    #[test]
    fn uma_fala_so_nao_basta_para_o_grupo() {
        let obs = every(0.0, 10.0, &grupo3(&["Ana Souza"]));
        let a = attribute(&obs, &[seg(1, 1.0, 4.0), seg(1, 30.0, 34.0)], &no_lag());
        assert_eq!(a.names, [None, None]);
        assert_eq!(a.report.thin_groups, 1);
    }

    #[test]
    fn so_as_falas_com_evidencia_no_trecho_recebem_o_nome() {
        let obs = every(0.0, 10.0, &grupo3(&["Ana Souza"]));
        // A terceira fala do grupo não tem leitura nenhuma.
        let a = attribute(
            &obs,
            &[seg(1, 1.0, 4.0), seg(1, 5.0, 9.0), seg(1, 40.0, 44.0)],
            &no_lag(),
        );
        assert_eq!(
            a.names,
            [Some("Ana Souza".into()), Some("Ana Souza".into()), None]
        );
    }

    #[test]
    fn menos_da_metade_da_fala_nao_conta() {
        let obs = every(0.0, 2.0, &grupo3(&["Ana Souza"]));
        // 3,5 s de leitura válida (até 1,5 s + 2 s de validade) numa fala de 10 s.
        let a = attribute(&obs, &[seg(1, 0.0, 10.0), seg(1, 0.0, 3.0)], &no_lag());
        assert_eq!(a.names[0], None);
    }

    #[test]
    fn chamada_de_dois_da_nome_sem_marca_de_quem_fala() {
        let r = reading(&[], &[ME, "José Antônio"], Some(ME));
        let obs = every(0.0, 20.0, &r);
        let a = attribute(&obs, &[seg(1, 1.0, 4.0), seg(1, 6.0, 12.0)], &no_lag());
        assert_eq!(
            a.names,
            [Some("José Antônio".into()), Some("José Antônio".into())]
        );
        // Sem a regra, nada.
        let off = Rules {
            two_party: false,
            ..no_lag()
        };
        assert_eq!(
            attribute(&obs, &[seg(1, 1.0, 4.0), seg(1, 6.0, 12.0)], &off).names,
            [None, None]
        );
    }

    #[test]
    fn chamada_de_dois_sem_saber_quem_e_voce_fica_generica() {
        let r = reading(&[], &[ME, "José Antônio"], None);
        let obs = every(0.0, 20.0, &r);
        assert_eq!(
            attribute(&obs, &[seg(1, 1.0, 4.0), seg(1, 6.0, 12.0)], &no_lag()).names,
            [None, None]
        );
    }

    #[test]
    fn chamada_de_dois_com_alguem_de_fora_marcado_fica_generica() {
        // Um nome que o painel ainda não mostra: não é a prova de que só
        // há duas pessoas.
        let r = reading(&["Visitante"], &[ME, "José Antônio"], Some(ME));
        let obs = every(0.0, 20.0, &r);
        assert_eq!(
            attribute(&obs, &[seg(1, 1.0, 4.0), seg(1, 6.0, 12.0)], &no_lag()).names,
            [None, None]
        );
    }

    #[test]
    fn homonimos_ficam_genericos() {
        let r = reading(&["Ana Souza"], &[ME, "Ana Souza", "Ana Souza"], Some(ME));
        let obs = every(0.0, 10.0, &r);
        assert_eq!(
            attribute(&obs, &[seg(1, 1.0, 4.0), seg(1, 5.0, 9.0)], &no_lag()).names,
            [None, None]
        );
        // Dois homônimos numa chamada "de dois" também.
        let r = reading(&[], &["Ana Souza", "Ana Souza"], Some("Ana Souza"));
        assert_eq!(
            attribute(
                &every(0.0, 10.0, &r),
                &[seg(1, 1.0, 4.0), seg(1, 5.0, 9.0)],
                &no_lag()
            )
            .names,
            [None, None]
        );
    }

    #[test]
    fn marcado_sem_painel_exige_a_regra_relaxada() {
        let r = reading(&["Ana Souza"], &[], None);
        let obs = every(0.0, 10.0, &r);
        let segs = [seg(1, 1.0, 4.0), seg(1, 5.0, 9.0)];
        assert_eq!(attribute(&obs, &segs, &no_lag()).names, [None, None]);
        let relaxed = Rules {
            require_roster: false,
            ..no_lag()
        };
        assert_eq!(
            attribute(&obs, &segs, &relaxed).names,
            [Some("Ana Souza".into()), Some("Ana Souza".into())]
        );
    }

    #[test]
    fn voce_nunca_vira_nome_dos_participantes() {
        // Só você marcado, numa chamada de três: nada.
        let obs = every(0.0, 10.0, &grupo3(&[ME]));
        let a = attribute(&obs, &[seg(1, 1.0, 4.0), seg(1, 5.0, 9.0)], &no_lag());
        assert_eq!(a.names, [None, None]);
        // E "Eu" não tem grupo.
        assert_eq!(group_of("Eu"), None);
        assert_eq!(group_of("Participantes"), None);
        assert_eq!(group_of("Tatiana"), None);
        assert_eq!(group_of("Participante 3"), Some(3));
    }

    #[test]
    fn leitura_invalida_corta_a_validade_da_anterior() {
        let mut obs = every(0.0, 3.0, &grupo3(&["Ana Souza"]));
        obs.push(Observation::gap(3.0, ObservationState::Unbound));
        obs.extend(every(20.0, 30.0, &grupo3(&["Ana Souza"])));
        let segs = [seg(1, 3.0, 5.0), seg(1, 0.5, 2.5), seg(1, 21.0, 24.0)];
        let a = attribute(&obs, &segs, &no_lag());
        assert_eq!(a.names[0], None, "a lacuna não herda o nome");
        assert_eq!(a.names[1], Some("Ana Souza".into()));
        for state in [
            ObservationState::NoCallWindow,
            ObservationState::NotRunning,
            ObservationState::Ambiguous,
            ObservationState::Error,
        ] {
            let obs: Vec<_> = every(0.0, 10.0, &grupo3(&["Ana Souza"]))
                .into_iter()
                .map(|o| Observation { state, ..o })
                .collect();
            assert_eq!(
                attribute(&obs, &[seg(1, 1.0, 4.0), seg(1, 5.0, 9.0)], &no_lag()).names,
                [None, None],
                "{state:?}"
            );
        }
    }

    #[test]
    fn falas_sem_diarizacao_nao_recebem_nome() {
        let obs = every(0.0, 10.0, &grupo3(&["Ana Souza"]));
        let segs = [
            RemoteSegment {
                group: None,
                start_secs: 1.0,
                end_secs: 4.0,
            },
            RemoteSegment {
                group: None,
                start_secs: 5.0,
                end_secs: 9.0,
            },
        ];
        let a = attribute(&obs, &segs, &no_lag());
        assert_eq!(a.names, [None, None]);
        assert_eq!(a.report.ungrouped_segments, 2);
    }

    #[test]
    fn fala_sem_duracao_usa_a_leitura_do_instante() {
        let obs = every(0.0, 10.0, &grupo3(&["Ana Souza"]));
        let a = attribute(&obs, &[seg(1, 2.0, 2.0), seg(1, 5.0, 9.0)], &no_lag());
        assert_eq!(
            a.names,
            [Some("Ana Souza".into()), Some("Ana Souza".into())]
        );
    }

    #[test]
    fn a_ordem_de_chegada_das_leituras_nao_muda_nada() {
        let mut obs = every(0.0, 10.0, &grupo3(&["Ana Souza"]));
        obs.extend(every(10.0, 20.0, &grupo3(&["João Ávila"])));
        let segs = [
            seg(1, 1.0, 4.0),
            seg(1, 5.0, 9.0),
            seg(2, 11.0, 14.0),
            seg(2, 15.0, 19.0),
        ];
        let base = attribute(&obs, &segs, &no_lag());
        let mut rev = obs.clone();
        rev.reverse();
        assert_eq!(attribute(&rev, &segs, &no_lag()), base);
        // Uma permutação fixa qualquer.
        let mut shuffled = obs.clone();
        shuffled.rotate_left(7);
        shuffled.swap(3, 30);
        assert_eq!(attribute(&shuffled, &segs, &no_lag()), base);
    }

    #[test]
    fn nomes_com_acento_e_caixa_diferente_sao_a_mesma_pessoa() {
        let mut obs = every(0.0, 5.0, &grupo3(&["João Ávila"]));
        obs.extend(every(
            5.0,
            10.0,
            &reading(&["JOÃO  ÁVILA"], &[ME, "Ana Souza", "JOÃO ÁVILA"], Some(ME)),
        ));
        let a = attribute(&obs, &[seg(1, 1.0, 4.0), seg(1, 6.0, 9.0)], &no_lag());
        assert_eq!(a.report.named_segments, 2, "{:?}", a.report);
        // As duas falas com a grafia da primeira leitura.
        assert_eq!(
            a.names,
            [Some("João Ávila".into()), Some("João Ávila".into())]
        );
    }

    #[test]
    fn relatorio_serializado_nao_tem_nomes() {
        let obs = every(0.0, 10.0, &grupo3(&["Ana Souza"]));
        let a = attribute(&obs, &[seg(1, 1.0, 4.0), seg(1, 5.0, 9.0)], &no_lag());
        let json = serde_json::to_string(&a.report).unwrap();
        assert!(!json.contains("Ana"), "{json}");
    }
}
