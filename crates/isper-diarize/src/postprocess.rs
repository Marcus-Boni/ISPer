//! Limpeza dos turnos do agrupamento — tudo função pura, testável sem ONNX.
//!
//! Duas regras, nesta ordem:
//!
//! 1. **absorção de grupos fracos**: um grupo com pouquíssimo tempo de fala
//!    (ou pouquíssimos turnos) não é um participante da reunião; é um trecho
//!    que o agrupamento não soube encaixar. Cada turno dele vai para o grupo
//!    forte mais próximo no tempo — a pessoa que estava falando ao redor;
//! 2. **renumeração por ordem de aparição**: quem fala primeiro é o 0. Sem
//!    isto os ids têm buracos e o leitor vê "Participante 1, 4, 9".
//!
//! O que a limpeza NÃO faz: esconder que houve problema. Tudo o que ela mexeu
//! sai em [`DiarizeMetrics`].

use crate::{DiarizeOptions, SpeakerTurn};

/// O que o agrupamento produziu e o que a limpeza fez com aquilo.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DiarizeMetrics {
    /// Grupos distintos criados pelo agrupamento, antes de qualquer limpeza.
    pub raw_clusters: usize,
    /// Turnos devolvidos pelo agrupamento.
    pub raw_turns: usize,
    /// Falantes publicados.
    pub speakers: usize,
    /// Turnos publicados (turnos vizinhos do mesmo falante viram um só).
    pub turns: usize,
    /// Grupos que a limpeza absorveu por serem fracos demais.
    pub absorbed_clusters: usize,
    /// Mediana da duração dos turnos publicados.
    pub median_turn_secs: f32,
    /// Turnos de menos de 500 ms — sintoma de fragmentação.
    pub very_short_turns: usize,
    /// Fala coberta pelos turnos publicados.
    pub speech_secs: f32,
    /// Duração do áudio analisado.
    pub audio_secs: f32,
    /// Quanto a diarização levou.
    pub elapsed_secs: f32,
}

/// Aplica as regras acima. Devolve os turnos limpos e as métricas.
pub fn postprocess(
    mut raw: Vec<SpeakerTurn>,
    opts: &DiarizeOptions,
) -> (Vec<SpeakerTurn>, DiarizeMetrics) {
    raw.sort_by(|a, b| a.start.total_cmp(&b.start));
    raw.retain(|t| t.secs() > 0.0);

    let mut metrics = DiarizeMetrics {
        raw_clusters: distinct(&raw),
        raw_turns: raw.len(),
        ..Default::default()
    };
    if raw.is_empty() {
        return (raw, metrics);
    }

    // 1. Quem é forte o bastante para ser um participante. O piso é o maior
    // entre um mínimo absoluto e uma fração da fala total — assim o critério
    // vale igual numa reunião de três minutos e numa de duas horas.
    let fala_total: f32 = raw.iter().map(SpeakerTurn::secs).sum();
    let piso = opts
        .min_speaker_secs
        .max(fala_total * opts.min_speaker_share.max(0.0));
    let mut ids: Vec<u32> = raw.iter().map(|t| t.speaker).collect();
    ids.sort_unstable();
    ids.dedup();
    let strong: Vec<u32> = ids
        .iter()
        .copied()
        .filter(|id| {
            let secs: f32 = raw
                .iter()
                .filter(|t| t.speaker == *id)
                .map(|t| t.secs())
                .sum();
            let turns = raw.iter().filter(|t| t.speaker == *id).count();
            secs >= piso && turns >= opts.min_speaker_turns
        })
        .collect();
    metrics.absorbed_clusters = ids.len() - strong.len();

    // Se NENHUM grupo é forte (áudio curtíssimo, ou tudo picotado), não há a
    // quem absorver: publica como veio, e a contagem absurda vira aviso lá
    // fora. Melhor um resultado honesto do que um inventado.
    if !strong.is_empty() && metrics.absorbed_clusters > 0 {
        absorb_weak(&mut raw, &strong);
    } else {
        metrics.absorbed_clusters = 0;
    }

    // 2. Turnos vizinhos que viraram o mesmo falante viram um turno só.
    let merged = merge_adjacent(raw, opts.min_duration_off);
    let renumbered = renumber(merged);

    metrics.speakers = distinct(&renumbered);
    metrics.turns = renumbered.len();
    metrics.speech_secs = renumbered.iter().map(SpeakerTurn::secs).sum();
    metrics.very_short_turns = renumbered.iter().filter(|t| t.secs() < 0.5).count();
    metrics.median_turn_secs = median(&renumbered);
    (renumbered, metrics)
}

fn distinct(turns: &[SpeakerTurn]) -> usize {
    let mut ids: Vec<u32> = turns.iter().map(|t| t.speaker).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.len()
}

fn median(turns: &[SpeakerTurn]) -> f32 {
    let mut d: Vec<f32> = turns.iter().map(SpeakerTurn::secs).collect();
    d.sort_by(f32::total_cmp);
    match d.len() {
        0 => 0.0,
        n if n % 2 == 1 => d[n / 2],
        n => (d[n / 2 - 1] + d[n / 2]) / 2.0,
    }
}

/// Cada turno de um grupo fraco vira o grupo forte mais próximo no tempo.
fn absorb_weak(turns: &mut [SpeakerTurn], strong: &[u32]) {
    // Índices dos turnos fortes, na ordem (a lista já está ordenada por início).
    let strong_idx: Vec<usize> = turns
        .iter()
        .enumerate()
        .filter(|(_, t)| strong.contains(&t.speaker))
        .map(|(i, _)| i)
        .collect();
    if strong_idx.is_empty() {
        return;
    }
    let novo: Vec<Option<u32>> = turns
        .iter()
        .enumerate()
        .map(|(i, t)| {
            if strong.contains(&t.speaker) {
                return None;
            }
            // Distância no tempo até o turno forte anterior e o seguinte.
            let antes = strong_idx.iter().rev().find(|k| **k < i).map(|k| {
                let d = (t.start - turns[*k].end).max(0.0);
                (d, turns[*k].speaker)
            });
            let depois = strong_idx.iter().find(|k| **k > i).map(|k| {
                let d = (turns[*k].start - t.end).max(0.0);
                (d, turns[*k].speaker)
            });
            match (antes, depois) {
                (Some(a), Some(b)) => Some(if a.0 <= b.0 { a.1 } else { b.1 }),
                (Some(a), None) => Some(a.1),
                (None, Some(b)) => Some(b.1),
                (None, None) => None,
            }
        })
        .collect();
    for (t, n) in turns.iter_mut().zip(novo) {
        if let Some(n) = n {
            t.speaker = n;
        }
    }
}

/// Junta turnos vizinhos do mesmo falante separados por menos de `gap`.
fn merge_adjacent(turns: Vec<SpeakerTurn>, gap: f32) -> Vec<SpeakerTurn> {
    let mut out: Vec<SpeakerTurn> = Vec::with_capacity(turns.len());
    for t in turns {
        match out.last_mut() {
            Some(prev) if prev.speaker == t.speaker && t.start - prev.end <= gap.max(0.0) => {
                prev.end = prev.end.max(t.end);
            }
            _ => out.push(t),
        }
    }
    out
}

/// Renumera por ordem de aparição: 0, 1, 2…
fn renumber(turns: Vec<SpeakerTurn>) -> Vec<SpeakerTurn> {
    let mut mapa: Vec<u32> = Vec::new();
    turns
        .into_iter()
        .map(|mut t| {
            let novo = match mapa.iter().position(|id| *id == t.speaker) {
                Some(i) => i,
                None => {
                    mapa.push(t.speaker);
                    mapa.len() - 1
                }
            };
            t.speaker = novo as u32;
            t
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(a: f32, b: f32, spk: u32) -> SpeakerTurn {
        SpeakerTurn {
            start: a,
            end: b,
            speaker: spk,
        }
    }

    fn opts() -> DiarizeOptions {
        DiarizeOptions {
            min_speaker_secs: 6.0,
            // Zerado nos testes de unidade: as fixtures são curtas e a fração
            // só entra em jogo em reunião longa, que tem teste próprio.
            min_speaker_share: 0.0,
            min_speaker_turns: 2,
            min_duration_off: 0.5,
            ..Default::default()
        }
    }

    #[test]
    fn o_piso_de_participante_acompanha_a_duracao_da_reuniao() {
        // Duas horas de reunião: dois falantes de verdade, alternando, mais um
        // grupo que somou 20 s em quatro aparições. Com piso fixo de 6 s ele
        // passava por participante; com 2% da fala (≈142 s), não.
        let mut raw = Vec::new();
        for i in 0..20 {
            let base = i as f32 * 360.0;
            raw.push(t(base, base + 180.0, 0));
            raw.push(t(base + 181.0, base + 355.0, 1));
        }
        for i in 0..4 {
            let base = 100.0 + i as f32 * 900.0;
            raw.push(t(base, base + 5.0, 2));
        }
        let longa = DiarizeOptions {
            min_speaker_share: 0.02,
            ..opts()
        };
        let (_, m) = postprocess(raw.clone(), &longa);
        assert_eq!(m.raw_clusters, 3);
        assert_eq!(m.absorbed_clusters, 1, "20 s em 2 h não é participante");
        assert_eq!(m.speakers, 2);

        // Com o piso só absoluto (o que havia antes), os mesmos 20 s passam.
        let antiga = DiarizeOptions {
            min_speaker_share: 0.0,
            ..opts()
        };
        let (_, m) = postprocess(raw, &antiga);
        assert_eq!(
            m.speakers, 3,
            "o piso fixo de 6 s deixa o grupo fraco passar"
        );
    }

    #[test]
    fn grupo_de_um_trecho_solto_e_absorvido_pelo_vizinho() {
        // Duas pessoas de verdade e um "participante" que falou 0,4 s uma vez.
        let raw = vec![
            t(0.0, 10.0, 0),
            t(10.5, 10.9, 7), // grupo espúrio
            t(11.0, 20.0, 0),
            t(21.0, 31.0, 1),
            t(32.0, 42.0, 1),
        ];
        let (turns, m) = postprocess(raw, &opts());
        assert_eq!(m.raw_clusters, 3);
        assert_eq!(m.absorbed_clusters, 1);
        assert_eq!(m.speakers, 2, "{turns:#?}");
        // E o trecho absorvido emendou com os vizinhos do mesmo falante.
        assert_eq!(turns.len(), 3, "{turns:#?}");
        assert!((turns[0].end - 20.0).abs() < 0.001);
    }

    #[test]
    fn falantes_saem_numerados_por_ordem_de_aparicao() {
        let raw = vec![
            t(0.0, 10.0, 9),
            t(11.0, 21.0, 4),
            t(22.0, 32.0, 9),
            t(33.0, 43.0, 4),
        ];
        let (turns, _) = postprocess(raw, &opts());
        assert_eq!(
            turns.iter().map(|t| t.speaker).collect::<Vec<_>>(),
            vec![0, 1, 0, 1]
        );
    }

    #[test]
    fn agrupamento_que_explodiu_nao_e_maquiado() {
        // 200 trechos, cada um no seu próprio grupo: nenhum é forte. A limpeza
        // não inventa um dono — a contagem absurda continua visível.
        let raw: Vec<SpeakerTurn> = (0..200)
            .map(|i| t(i as f32, i as f32 + 0.4, i as u32))
            .collect();
        let (turns, m) = postprocess(raw, &opts());
        assert_eq!(m.raw_clusters, 200);
        assert_eq!(m.absorbed_clusters, 0, "não há forte a quem absorver");
        assert_eq!(m.speakers, 200, "{}", turns.len());
        assert!(m.very_short_turns > 100);
    }

    #[test]
    fn turnos_vizinhos_do_mesmo_falante_viram_um() {
        let raw = vec![t(0.0, 5.0, 0), t(5.2, 10.0, 0), t(10.1, 15.0, 0)];
        let (turns, m) = postprocess(raw, &opts());
        assert_eq!(turns.len(), 1);
        assert_eq!(m.turns, 1);
        assert!((turns[0].end - 15.0).abs() < 0.001);
        assert!((m.speech_secs - 15.0).abs() < 0.001);
    }

    #[test]
    fn pausa_longa_nao_emenda_turnos() {
        let raw = vec![t(0.0, 5.0, 0), t(30.0, 35.0, 0)];
        let (turns, _) = postprocess(raw, &opts());
        assert_eq!(turns.len(), 2);
    }

    #[test]
    fn metricas_de_mediana_e_turnos_curtos() {
        let raw = vec![
            t(0.0, 10.0, 0),
            t(20.0, 22.0, 1),
            t(30.0, 30.2, 0),
            t(40.0, 48.0, 1),
        ];
        let o = DiarizeOptions {
            min_speaker_secs: 0.0,
            min_speaker_turns: 0,
            ..opts()
        };
        let (_, m) = postprocess(raw, &o);
        assert_eq!(m.speakers, 2);
        assert_eq!(m.very_short_turns, 1);
        // Durações: 0,2 · 2 · 8 · 10 → mediana (2 + 8) / 2 = 5.
        assert!((m.median_turn_secs - 5.0).abs() < 0.001, "{m:?}");
    }

    #[test]
    fn nada_entra_nada_sai() {
        let (turns, m) = postprocess(Vec::new(), &opts());
        assert!(turns.is_empty());
        assert_eq!(m.speakers, 0);
        assert_eq!(m.raw_clusters, 0);
    }

    #[test]
    fn turno_degenerado_e_descartado() {
        let raw = vec![t(5.0, 5.0, 0), t(0.0, 10.0, 1), t(11.0, 21.0, 1)];
        let (turns, m) = postprocess(raw, &opts());
        assert_eq!(m.raw_turns, 2);
        assert_eq!(turns.len(), 2);
    }
}
