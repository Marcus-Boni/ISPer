//! Alinhamento temporal: de palavras + turnos de diarização para falas.
//!
//! O jeito antigo era atribuir o falante por SEGMENTO do Whisper: um segmento
//! de 10 s recebia inteiro o falante de maior sobreposição. Quando duas
//! pessoas falam dentro do mesmo segmento — o caso comum de "concordo, mas
//! mudaria o prazo" logo depois de uma frase —, metade da fala vai para a
//! pessoa errada.
//!
//! Aqui a unidade é a PALAVRA (timestamps vêm do `token_timestamps` do
//! Whisper), e a fala é reconstruída agrupando palavras vizinhas do mesmo
//! falante. Tudo neste módulo é função pura sobre dados — sem modelo, sem
//! áudio, sem E/S —, então cada regra de fronteira tem teste.

use serde::{Deserialize, Serialize};

use crate::engine::Word;

/// Identificador de falante vindo do agrupamento da diarização.
///
/// `u32` e não `u8`: o `u8` obrigava a saturar em 255 e produzia o famoso
/// "Participante 255" quando o agrupamento explodia. O número absurdo agora
/// aparece nas métricas e é tratado por [`crate::diarization`], em vez de
/// virar um rótulo silencioso.
pub type SpeakerId = u32;

/// Um turno de fala: `[start, end)` em segundos, e quem falou.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeakerTurn {
    pub start_secs: f32,
    pub end_secs: f32,
    pub speaker: SpeakerId,
}

impl SpeakerTurn {
    pub fn secs(&self) -> f32 {
        (self.end_secs - self.start_secs).max(0.0)
    }
}

/// Uma palavra já com dono.
#[derive(Debug, Clone, PartialEq)]
pub struct TaggedWord {
    pub word: Word,
    /// `None` quando nenhum turno cobre a palavra (silêncio do diarizador,
    /// fala sobreposta descartada, áudio fora das regiões analisadas).
    pub speaker: Option<SpeakerId>,
}

/// Uma fala reconstruída: palavras consecutivas do mesmo falante.
#[derive(Debug, Clone, PartialEq)]
pub struct Utterance {
    pub speaker: Option<SpeakerId>,
    pub start_secs: f32,
    pub end_secs: f32,
    pub text: String,
    /// Quantas palavras entraram — usado para descartar falas de uma palavra
    /// solta atribuídas a um falante que não existe.
    pub words: usize,
}

impl Utterance {
    pub fn secs(&self) -> f32 {
        (self.end_secs - self.start_secs).max(0.0)
    }
}

/// Regras da reconstrução das falas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignOptions {
    /// Distância máxima entre uma palavra e o turno mais próximo para ainda
    /// assim herdar o falante dele. Acima disso a palavra fica sem dono.
    pub max_snap_secs: f32,
    /// Trocas de falante mais curtas que isto, cercadas pelo mesmo falante
    /// dos dois lados, são ruído do diarizador — a palavra volta para o
    /// falante de fora. É o que evita "João / Maria / João" no meio de uma
    /// frase única.
    pub min_switch_secs: f32,
    /// Pausa que encerra a fala mesmo sem troca de falante.
    pub max_gap_secs: f32,
    /// Teto de uma fala, para o texto não virar parede.
    pub max_utterance_secs: f32,
}

impl Default for AlignOptions {
    fn default() -> Self {
        Self {
            max_snap_secs: 0.75,
            min_switch_secs: 1.0,
            max_gap_secs: 2.0,
            max_utterance_secs: 60.0,
        }
    }
}

/// Dá dono a cada palavra pelo turno de maior sobreposição.
///
/// Sem sobreposição nenhuma, a palavra "encosta" no turno mais próximo se ele
/// estiver a menos de `max_snap_secs` — caso contrário fica sem dono, o que é
/// honesto: melhor "Participantes" do que atribuir a pessoa errada.
pub fn tag_words(words: &[Word], turns: &[SpeakerTurn], opts: &AlignOptions) -> Vec<TaggedWord> {
    let mut out: Vec<TaggedWord> = words
        .iter()
        .map(|w| TaggedWord {
            speaker: speaker_for(w.start_secs, w.end_secs, turns, opts.max_snap_secs),
            word: w.clone(),
        })
        .collect();
    smooth_switches(&mut out, opts.min_switch_secs);
    out
}

/// Falante de um intervalo: maior sobreposição; empate resolvido pelo turno
/// que começa antes (ordem estável, sem depender da ordem da entrada).
fn speaker_for(
    start: f32,
    end: f32,
    turns: &[SpeakerTurn],
    max_snap_secs: f32,
) -> Option<SpeakerId> {
    let end = end.max(start);
    let mut best: Option<(f32, f32, SpeakerId)> = None; // (sobreposição, -início, id)
    for t in turns {
        let overlap = (t.end_secs.min(end) - t.start_secs.max(start)).max(0.0);
        if overlap <= 0.0 {
            continue;
        }
        let cand = (overlap, -t.start_secs, t.speaker);
        if best.is_none_or(|b| (cand.0, cand.1) > (b.0, b.1)) {
            best = Some(cand);
        }
    }
    if let Some((_, _, spk)) = best {
        return Some(spk);
    }
    // Nada sobrepõe: encosta no turno mais próximo, se estiver perto.
    let mut nearest: Option<(f32, SpeakerId)> = None;
    for t in turns {
        let dist = if end < t.start_secs {
            t.start_secs - end
        } else if start > t.end_secs {
            start - t.end_secs
        } else {
            0.0
        };
        if dist <= max_snap_secs && nearest.is_none_or(|(d, _)| dist < d) {
            nearest = Some((dist, t.speaker));
        }
    }
    nearest.map(|(_, spk)| spk)
}

/// Apaga trocas de falante curtas demais para serem reais.
///
/// Uma palavra (ou duas) com falante diferente, cercada pelo MESMO falante
/// dos dois lados e durando menos que `min_switch_secs`, é quase sempre ruído
/// do diarizador numa fronteira de turno.
fn smooth_switches(words: &mut [TaggedWord], min_switch_secs: f32) {
    if words.len() < 3 || min_switch_secs <= 0.0 {
        return;
    }
    let mut i = 0;
    while i < words.len() {
        let Some(spk) = words[i].speaker else {
            i += 1;
            continue;
        };
        // Extensão do trecho com o mesmo falante.
        let mut j = i;
        while j + 1 < words.len() && words[j + 1].speaker == Some(spk) {
            j += 1;
        }
        let before = i.checked_sub(1).and_then(|k| words[k].speaker);
        let after = words.get(j + 1).and_then(|w| w.speaker);
        let secs = words[j].word.end_secs - words[i].word.start_secs;
        if before.is_some() && before == after && before != Some(spk) && secs < min_switch_secs {
            let vizinho = before;
            for w in &mut words[i..=j] {
                w.speaker = vizinho;
            }
        }
        i = j + 1;
    }
}

/// Junta palavras vizinhas do mesmo falante numa fala.
pub fn build_utterances(words: &[TaggedWord], opts: &AlignOptions) -> Vec<Utterance> {
    let mut out: Vec<Utterance> = Vec::new();
    for tw in words {
        let text = tw.word.text.trim();
        if text.is_empty() {
            continue;
        }
        let continues = out.last().is_some_and(|u| {
            u.speaker == tw.speaker
                && tw.word.start_secs - u.end_secs < opts.max_gap_secs
                && tw.word.end_secs - u.start_secs <= opts.max_utterance_secs
        });
        match out.last_mut() {
            Some(u) if continues => {
                push_word(&mut u.text, text);
                u.end_secs = u.end_secs.max(tw.word.end_secs);
                u.words += 1;
            }
            _ => out.push(Utterance {
                speaker: tw.speaker,
                start_secs: tw.word.start_secs,
                end_secs: tw.word.end_secs.max(tw.word.start_secs),
                text: text.to_string(),
                words: 1,
            }),
        }
    }
    out
}

/// Emenda uma palavra no texto sem espaço antes de pontuação — o Whisper
/// devolve "," e "." como tokens próprios.
fn push_word(buf: &mut String, word: &str) {
    let glue = !word.starts_with([',', '.', '!', '?', ';', ':', '…', ')', ']', '%']);
    if glue && !buf.is_empty() {
        buf.push(' ');
    }
    buf.push_str(word);
}

/// Métricas de "quem falou o quê" — o que permite dizer se a diarização
/// melhorou ou piorou sem ouvir a reunião inteira.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeakerMetrics {
    /// Falantes distintos nas falas finais.
    pub speakers: usize,
    /// Quantas vezes o falante mudou de uma fala para a próxima.
    pub switches: usize,
    /// Mediana da duração das falas.
    pub median_turn_secs: f32,
    /// Falas com menos de 500 ms — o sintoma clássico de fragmentação.
    pub very_short_turns: usize,
    /// Falas sem falante atribuído.
    pub unassigned: usize,
}

pub fn speaker_metrics(utterances: &[Utterance]) -> SpeakerMetrics {
    let mut ids: Vec<SpeakerId> = utterances.iter().filter_map(|u| u.speaker).collect();
    ids.sort_unstable();
    ids.dedup();
    let switches = utterances
        .windows(2)
        .filter(|w| w[0].speaker != w[1].speaker)
        .count();
    let mut durations: Vec<f32> = utterances.iter().map(Utterance::secs).collect();
    durations.sort_by(f32::total_cmp);
    let median_turn_secs = match durations.len() {
        0 => 0.0,
        n if n % 2 == 1 => durations[n / 2],
        n => (durations[n / 2 - 1] + durations[n / 2]) / 2.0,
    };
    SpeakerMetrics {
        speakers: ids.len(),
        switches,
        median_turn_secs,
        very_short_turns: durations.iter().filter(|d| **d < 0.5).count(),
        unassigned: utterances.iter().filter(|u| u.speaker.is_none()).count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(text: &str, a: f32, b: f32) -> Word {
        Word {
            text: text.into(),
            start_secs: a,
            end_secs: b,
            prob: 0.9,
        }
    }

    fn t(a: f32, b: f32, spk: SpeakerId) -> SpeakerTurn {
        SpeakerTurn {
            start_secs: a,
            end_secs: b,
            speaker: spk,
        }
    }

    #[test]
    fn duas_pessoas_no_mesmo_segmento_ganham_falas_separadas() {
        // O caso que a atribuição por segmento errava: o segmento inteiro ia
        // para quem tivesse mais tempo.
        let words = [
            w("Acho", 10.0, 10.4),
            w("que", 10.4, 10.7),
            w("precisamos", 10.7, 11.5),
            w("disso", 11.5, 12.0),
            w("Concordo", 15.2, 15.9),
            w(",", 15.9, 16.0),
            w("mas", 16.0, 16.3),
            w("mudaria", 16.3, 17.0),
            w("o", 17.0, 17.1),
            w("prazo", 17.1, 17.7),
        ];
        let turns = [t(10.0, 14.8, 1), t(15.0, 19.0, 2)];
        let tagged = tag_words(&words, &turns, &AlignOptions::default());
        let falas = build_utterances(&tagged, &AlignOptions::default());
        assert_eq!(falas.len(), 2, "{falas:#?}");
        assert_eq!(falas[0].speaker, Some(1));
        assert_eq!(falas[0].text, "Acho que precisamos disso");
        assert_eq!(falas[1].speaker, Some(2));
        assert_eq!(falas[1].text, "Concordo, mas mudaria o prazo");
    }

    #[test]
    fn troca_curta_no_meio_da_frase_e_alisada() {
        // O diarizador pisca em "de": sem alisar, a fala quebra em três.
        let words = [
            w("o", 0.0, 0.2),
            w("fluxo", 0.2, 0.7),
            w("de", 0.7, 0.9),
            w("caixa", 0.9, 1.5),
            w("fechou", 1.5, 2.0),
        ];
        let turns = [t(0.0, 0.7, 1), t(0.7, 0.9, 7), t(0.9, 2.0, 1)];
        let tagged = tag_words(&words, &turns, &AlignOptions::default());
        assert!(
            tagged.iter().all(|t| t.speaker == Some(1)),
            "{:?}",
            tagged.iter().map(|t| t.speaker).collect::<Vec<_>>()
        );
        let falas = build_utterances(&tagged, &AlignOptions::default());
        assert_eq!(falas.len(), 1);
        assert_eq!(falas[0].text, "o fluxo de caixa fechou");
    }

    #[test]
    fn troca_longa_de_verdade_nao_e_alisada() {
        let words = [
            w("bom", 0.0, 0.4),
            w("dia", 0.4, 0.8),
            w("tudo", 2.0, 2.4),
            w("certo", 2.4, 3.0),
            w("por", 3.0, 3.3),
            w("aqui", 3.3, 3.8),
            w("obrigado", 5.0, 5.8),
        ];
        let turns = [t(0.0, 1.0, 1), t(1.9, 4.0, 2), t(4.9, 6.0, 1)];
        let tagged = tag_words(&words, &turns, &AlignOptions::default());
        let falas = build_utterances(&tagged, &AlignOptions::default());
        assert_eq!(falas.len(), 3, "{falas:#?}");
        assert_eq!(
            falas.iter().map(|f| f.speaker).collect::<Vec<_>>(),
            vec![Some(1), Some(2), Some(1)]
        );
    }

    #[test]
    fn palavra_fora_de_qualquer_turno_fica_sem_dono() {
        let words = [w("alo", 30.0, 30.5)];
        let turns = [t(0.0, 5.0, 1)];
        let tagged = tag_words(&words, &turns, &AlignOptions::default());
        assert_eq!(tagged[0].speaker, None);
    }

    #[test]
    fn palavra_quase_encostada_no_turno_herda_o_falante() {
        // Fronteira do VAD desloca a palavra em 0,3 s — ainda é a mesma pessoa.
        let words = [w("certo", 5.2, 5.6)];
        let turns = [t(0.0, 5.0, 3)];
        let tagged = tag_words(&words, &turns, &AlignOptions::default());
        assert_eq!(tagged[0].speaker, Some(3));
    }

    #[test]
    fn pausa_longa_quebra_a_fala_mesmo_sem_trocar_de_falante() {
        let words = [w("primeiro", 0.0, 0.8), w("ponto", 10.0, 10.6)];
        let turns = [t(0.0, 20.0, 1)];
        let tagged = tag_words(&words, &turns, &AlignOptions::default());
        let falas = build_utterances(&tagged, &AlignOptions::default());
        assert_eq!(falas.len(), 2, "{falas:#?}");
    }

    #[test]
    fn pontuacao_cola_sem_espaco() {
        let words = [
            w("sim", 0.0, 0.3),
            w(",", 0.3, 0.35),
            w("claro", 0.35, 0.8),
            w(".", 0.8, 0.85),
        ];
        let turns = [t(0.0, 1.0, 1)];
        let tagged = tag_words(&words, &turns, &AlignOptions::default());
        let falas = build_utterances(&tagged, &AlignOptions::default());
        assert_eq!(falas[0].text, "sim, claro.");
    }

    #[test]
    fn metricas_contam_trocas_curtas_e_sem_dono() {
        let u = |spk, a: f32, b: f32| Utterance {
            speaker: spk,
            start_secs: a,
            end_secs: b,
            text: "x".into(),
            words: 1,
        };
        let falas = [
            u(Some(1), 0.0, 4.0),
            u(Some(2), 4.0, 4.2), // 200 ms
            u(Some(1), 5.0, 9.0),
            u(None, 9.0, 12.0),
        ];
        let m = speaker_metrics(&falas);
        assert_eq!(m.speakers, 2);
        assert_eq!(m.switches, 3);
        assert_eq!(m.very_short_turns, 1);
        assert_eq!(m.unassigned, 1);
        assert!((m.median_turn_secs - 3.5).abs() < 0.001, "{m:?}");
    }

    #[test]
    fn sem_turnos_nada_ganha_dono() {
        let words = [w("oi", 0.0, 0.3)];
        let tagged = tag_words(&words, &[], &AlignOptions::default());
        assert_eq!(tagged[0].speaker, None);
        let falas = build_utterances(&tagged, &AlignOptions::default());
        assert_eq!(falas.len(), 1);
        assert_eq!(falas[0].speaker, None);
    }
}
