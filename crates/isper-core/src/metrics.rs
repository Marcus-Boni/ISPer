//! Métricas do pipeline: o relatório de uma rodada e as taxas de erro.
//!
//! O princípio do trabalho é "medir antes de otimizar". Para isso, cada
//! rodada do passe final produz um [`PipelineReport`] serializável: modelo,
//! parâmetros, tempos por etapa e contagens. Duas rodadas viram dois JSONs, e
//! a comparação é objetiva.
//!
//! As taxas de erro ([`wer`], [`cer`], [`der`]) ficam aqui porque só fazem
//! sentido junto do relatório — e porque precisam de teste, que é o único
//! jeito de confiar num número.

use serde::{Deserialize, Serialize};

use crate::align::{AlignOptions, SpeakerMetrics, SpeakerTurn};
use crate::profile::{DecodeConfig, TranscriptionProfile};
use crate::vad::{VadOptions, WindowOptions};

/// Tempos de cada etapa, em segundos.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StageTimings {
    pub vad_secs: f32,
    pub asr_secs: f32,
    pub diarize_secs: f32,
    pub align_secs: f32,
    pub total_secs: f32,
}

/// O que o VAD encontrou.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VadStats {
    pub regions: usize,
    pub speech_secs: f32,
    /// Silêncio que não foi ao Whisper — economia direta de GPU e a maior
    /// defesa contra alucinação (o Whisper inventa legenda em silêncio).
    pub discarded_silence_secs: f32,
    pub windows: usize,
    pub median_window_secs: f32,
}

/// O que o ASR produziu.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AsrStats {
    pub segments: usize,
    pub words: usize,
    /// Média das log-probabilidades dos segmentos (quanto o modelo confiou).
    pub avg_logprob: f32,
    /// Segmentos jogados fora pelo filtro de alucinações.
    pub dropped_segments: usize,
    /// Janelas que não produziram texto nenhum.
    pub empty_windows: usize,
    /// Janelas em que a inferência falhou (a reunião continua sem elas).
    pub failed_windows: usize,
    /// Janelas que herdaram contexto da anterior.
    pub windows_with_context: usize,
    pub segments_under_1s: usize,
    pub median_segment_secs: f32,
}

/// O que a diarização produziu (espelha `isper_diarize::DiarizeMetrics`, que
/// o core não pode importar sem puxar o sherpa junto).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DiarizeStats {
    pub raw_clusters: usize,
    pub raw_turns: usize,
    pub speakers: usize,
    pub turns: usize,
    pub absorbed_clusters: usize,
    pub median_turn_secs: f32,
    pub very_short_turns: usize,
    pub warnings: Vec<String>,
}

/// Todos os parâmetros que produziram esta rodada.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunParams {
    pub profile: TranscriptionProfile,
    pub lang: String,
    pub decode: DecodeConfig,
    pub vad: Option<VadOptions>,
    pub windows: WindowOptions,
    pub align: AlignOptions,
    pub context_chars: usize,
}

/// O relatório de uma rodada completa.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineReport {
    /// Nome livre da configuração ("baseline", "experimental"…).
    pub config: String,
    pub model: String,
    /// "cuda" ou "cpu" — de qual build o binário veio.
    pub device: String,
    /// Alinhamento DTW dos timestamps por token estava ligado.
    pub dtw: bool,
    pub audio_secs: f32,
    /// Tempo de processamento ÷ duração do áudio. Menor que 1 = mais rápido
    /// que o tempo real.
    pub realtime_factor: f32,
    pub timings: StageTimings,
    pub params: RunParams,
    pub vad: VadStats,
    pub asr: AsrStats,
    pub diarization: Option<DiarizeStats>,
    pub speakers: SpeakerMetrics,
}

impl PipelineReport {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|e| format!("{{\"erro\":\"{e}\"}}"))
    }
}

/// Em qual build estamos — vai para todo relatório.
pub const fn device() -> &'static str {
    if cfg!(feature = "cuda") {
        "cuda"
    } else {
        "cpu"
    }
}

// ------------------------------------------------------------ taxas de erro

/// Como normalizar os textos antes de comparar.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Normalization {
    pub lowercase: bool,
    pub strip_punct: bool,
    /// Ignorar acentos. Desligado por padrão: em português, "e" e "é" são
    /// palavras diferentes — apagar o acento esconde erro de verdade.
    pub strip_accents: bool,
}

impl Default for Normalization {
    fn default() -> Self {
        Self {
            lowercase: true,
            strip_punct: true,
            strip_accents: false,
        }
    }
}

impl Normalization {
    fn apply(&self, s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            let c = if self.strip_accents {
                crate::text::fold(&c.to_string())
                    .chars()
                    .next()
                    .unwrap_or(c)
            } else if self.lowercase {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                c
            };
            let c = if self.lowercase {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                c
            };
            if self.strip_punct && is_droppable_punct(c) {
                out.push(' ');
            } else {
                out.push(c);
            }
        }
        out
    }

    /// Palavras normalizadas.
    pub fn words(&self, s: &str) -> Vec<String> {
        self.apply(s)
            .split_whitespace()
            .map(str::to_string)
            .collect()
    }

    /// Caracteres normalizados, com os espaços colapsados.
    pub fn chars(&self, s: &str) -> Vec<char> {
        self.words(s).join(" ").chars().collect()
    }
}

fn is_droppable_punct(c: char) -> bool {
    matches!(
        c,
        '.' | ','
            | ';'
            | ':'
            | '!'
            | '?'
            | '"'
            | '\''
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '…'
            | '—'
            | '–'
            | '“'
            | '”'
            | '‘'
            | '’'
            | '«'
            | '»'
    )
}

/// Erros de uma comparação, abertos por tipo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ErrorRate {
    pub substitutions: usize,
    pub deletions: usize,
    pub insertions: usize,
    /// Unidades da referência (palavras ou caracteres).
    pub reference_len: usize,
    /// `(S + D + I) / N`. Pode passar de 1 quando há muita inserção.
    pub rate: f32,
}

impl ErrorRate {
    pub fn errors(&self) -> usize {
        self.substitutions + self.deletions + self.insertions
    }
}

/// Word Error Rate entre uma referência corrigida à mão e a hipótese.
pub fn wer(reference: &str, hypothesis: &str, norm: &Normalization) -> ErrorRate {
    let r = norm.words(reference);
    let h = norm.words(hypothesis);
    edit_stats(&r, &h)
}

/// Character Error Rate — mais sensível a acento, número e grafia de nome
/// próprio, que é justamente o que reunião corporativa precisa acertar.
pub fn cer(reference: &str, hypothesis: &str, norm: &Normalization) -> ErrorRate {
    let r = norm.chars(reference);
    let h = norm.chars(hypothesis);
    edit_stats(&r, &h)
}

/// Distância de edição com o detalhe de substituição/remoção/inserção.
///
/// Duas linhas de DP: uma reunião de duas horas tem ~20 mil palavras de cada
/// lado, e a matriz inteira (400 milhões de células) não cabe na memória.
fn edit_stats<T: PartialEq>(reference: &[T], hypothesis: &[T]) -> ErrorRate {
    #[derive(Clone, Copy, Default)]
    struct Cell {
        s: u32,
        d: u32,
        i: u32,
    }
    impl Cell {
        fn total(&self) -> u32 {
            self.s + self.d + self.i
        }
    }

    let n = reference.len();
    let m = hypothesis.len();
    let mut prev: Vec<Cell> = (0..=m)
        .map(|j| Cell {
            i: j as u32,
            ..Default::default()
        })
        .collect();
    let mut cur: Vec<Cell> = vec![Cell::default(); m + 1];

    for i in 1..=n {
        cur[0] = Cell {
            d: i as u32,
            ..Default::default()
        };
        for j in 1..=m {
            if reference[i - 1] == hypothesis[j - 1] {
                cur[j] = prev[j - 1];
                continue;
            }
            let sub = Cell {
                s: prev[j - 1].s + 1,
                ..prev[j - 1]
            };
            let del = Cell {
                d: prev[j].d + 1,
                ..prev[j]
            };
            let ins = Cell {
                i: cur[j - 1].i + 1,
                ..cur[j - 1]
            };
            // Empate resolvido na ordem substituição → remoção → inserção:
            // determinístico, que é o que um benchmark precisa.
            cur[j] = if sub.total() <= del.total() && sub.total() <= ins.total() {
                sub
            } else if del.total() <= ins.total() {
                del
            } else {
                ins
            };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    let c = prev[m];
    ErrorRate {
        substitutions: c.s as usize,
        deletions: c.d as usize,
        insertions: c.i as usize,
        reference_len: n,
        rate: if n == 0 {
            if m == 0 { 0.0 } else { 1.0 }
        } else {
            c.total() as f32 / n as f32
        },
    }
}

// ---------------------------------------------------------------------- DER

/// Diarization Error Rate por quadro, com o mapeamento ótimo de falantes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct DiarizationErrorRate {
    /// Fala da referência que ninguém cobriu.
    pub missed_secs: f32,
    /// Fala atribuída onde a referência diz que havia silêncio.
    pub false_alarm_secs: f32,
    /// Fala coberta, mas pelo falante errado.
    pub confusion_secs: f32,
    /// Fala total na referência.
    pub reference_secs: f32,
    pub rate: f32,
}

/// Calcula o DER comparando turnos de referência com os do sistema.
///
/// Discretiza em quadros de 10 ms e procura o casamento de falantes que
/// minimiza a confusão (permutações até 8 falantes; acima disso, guloso — um
/// número aproximado ainda serve para acompanhar tendência).
pub fn der(reference: &[SpeakerTurn], hypothesis: &[SpeakerTurn]) -> DiarizationErrorRate {
    const FRAME: f32 = 0.01;
    let end = reference
        .iter()
        .chain(hypothesis)
        .map(|t| t.end_secs)
        .fold(0.0f32, f32::max);
    if end <= 0.0 {
        return DiarizationErrorRate::default();
    }
    let frames = (end / FRAME).ceil() as usize;
    let ref_at = frame_labels(reference, frames, FRAME);
    let hyp_at = frame_labels(hypothesis, frames, FRAME);

    let ref_ids = distinct(&ref_at);
    let hyp_ids = distinct(&hyp_at);
    let mapping = best_mapping(&ref_at, &hyp_at, &ref_ids, &hyp_ids);

    let (mut missed, mut false_alarm, mut confusion, mut reference_frames) = (0usize, 0, 0, 0);
    for f in 0..frames {
        match (ref_at[f], hyp_at[f]) {
            (None, None) => {}
            (None, Some(_)) => false_alarm += 1,
            (Some(_), None) => {
                reference_frames += 1;
                missed += 1;
            }
            (Some(r), Some(h)) => {
                reference_frames += 1;
                if mapping.get(&r).copied() != Some(h) {
                    confusion += 1;
                }
            }
        }
    }
    let reference_secs = reference_frames as f32 * FRAME;
    let errors = (missed + false_alarm + confusion) as f32 * FRAME;
    DiarizationErrorRate {
        missed_secs: missed as f32 * FRAME,
        false_alarm_secs: false_alarm as f32 * FRAME,
        confusion_secs: confusion as f32 * FRAME,
        reference_secs,
        rate: if reference_secs > 0.0 {
            errors / reference_secs
        } else {
            0.0
        },
    }
}

/// Quem fala em cada quadro (o último turno vence uma sobreposição).
fn frame_labels(turns: &[SpeakerTurn], frames: usize, frame: f32) -> Vec<Option<u32>> {
    let mut out = vec![None; frames];
    for t in turns {
        let a = ((t.start_secs / frame).floor().max(0.0) as usize).min(frames);
        let b = ((t.end_secs / frame).ceil().max(0.0) as usize).min(frames);
        for slot in out.iter_mut().take(b).skip(a) {
            *slot = Some(t.speaker);
        }
    }
    out
}

fn distinct(labels: &[Option<u32>]) -> Vec<u32> {
    let mut ids: Vec<u32> = labels.iter().flatten().copied().collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Casamento referência → hipótese que maximiza os quadros corretos.
fn best_mapping(
    ref_at: &[Option<u32>],
    hyp_at: &[Option<u32>],
    ref_ids: &[u32],
    hyp_ids: &[u32],
) -> std::collections::HashMap<u32, u32> {
    use std::collections::HashMap;
    // Matriz de coincidência: quantos quadros o par (r, h) divide.
    let mut score: HashMap<(u32, u32), usize> = HashMap::new();
    for (r, h) in ref_at.iter().zip(hyp_at) {
        if let (Some(r), Some(h)) = (r, h) {
            *score.entry((*r, *h)).or_default() += 1;
        }
    }
    let get = |r: u32, h: u32| score.get(&(r, h)).copied().unwrap_or(0);

    // Até 8 falantes na referência, testa todas as permutações de hipóteses.
    if ref_ids.len() <= 8 && hyp_ids.len() <= 8 {
        let mut best: Option<(usize, HashMap<u32, u32>)> = None;
        let mut chosen: Vec<u32> = Vec::new();
        assign(ref_ids, hyp_ids, &mut chosen, &mut |pick: &[u32]| {
            let total: usize = ref_ids.iter().zip(pick).map(|(r, h)| get(*r, *h)).sum();
            if best.as_ref().is_none_or(|(b, _)| total > *b) {
                best = Some((
                    total,
                    ref_ids.iter().copied().zip(pick.iter().copied()).collect(),
                ));
            }
        });
        if let Some((_, m)) = best {
            return m;
        }
    }
    // Guloso: cada referência fica com a hipótese que mais divide quadros.
    let mut used: Vec<u32> = Vec::new();
    let mut out = HashMap::new();
    for r in ref_ids {
        if let Some(h) = hyp_ids
            .iter()
            .filter(|h| !used.contains(h))
            .max_by_key(|h| get(*r, **h))
        {
            used.push(*h);
            out.insert(*r, *h);
        }
    }
    out
}

/// Enumera as atribuições injetivas de `hyp` para cada posição de `refs`.
fn assign(refs: &[u32], hyp: &[u32], chosen: &mut Vec<u32>, visit: &mut impl FnMut(&[u32])) {
    if chosen.len() == refs.len() {
        visit(chosen);
        return;
    }
    for h in hyp {
        if chosen.contains(h) {
            continue;
        }
        chosen.push(*h);
        assign(refs, hyp, chosen, visit);
        chosen.pop();
    }
    // Menos hipóteses que referências: as sobrando ficam sem par.
    if hyp.len() < refs.len() && chosen.len() == hyp.len() {
        visit(chosen);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm() -> Normalization {
        Normalization::default()
    }

    #[test]
    fn textos_iguais_nao_tem_erro() {
        let e = wer(
            "o fluxo de caixa fechou",
            "O fluxo de caixa fechou.",
            &norm(),
        );
        assert_eq!(e.errors(), 0);
        assert_eq!(e.rate, 0.0);
        assert_eq!(e.reference_len, 5);
    }

    #[test]
    fn wer_abre_cada_tipo_de_erro() {
        // Um erro de cada vez, para o tipo ser inequívoco (com vários erros
        // no mesmo trecho há alinhamentos de custo igual, e aí só o TOTAL é
        // uma propriedade do par de textos).
        let so_substituicao = wer("o manual do fluxo", "o anual do fluxo", &norm());
        assert_eq!(so_substituicao.substitutions, 1, "manual → anual");
        assert_eq!(so_substituicao.deletions + so_substituicao.insertions, 0);

        let so_remocao = wer("o manual do fluxo", "o manual fluxo", &norm());
        assert_eq!(so_remocao.deletions, 1, "do sumiu");
        assert_eq!(so_remocao.substitutions + so_remocao.insertions, 0);

        let so_insercao = wer("o manual do fluxo", "o manual do fluxo hoje", &norm());
        assert_eq!(so_insercao.insertions, 1, "hoje sobrou");
        assert_eq!(so_insercao.substitutions + so_insercao.deletions, 0);
    }

    #[test]
    fn wer_total_e_a_distancia_de_edicao() {
        // "manual" → "anual", "de" some, "hoje" sobra: três edições.
        let e = wer(
            "o manual do fluxo de caixa",
            "o anual do fluxo caixa hoje",
            &norm(),
        );
        assert_eq!(e.errors(), 3, "{e:?}");
        assert_eq!(e.reference_len, 6);
        assert!((e.rate - 0.5).abs() < 0.001, "{e:?}");
    }

    #[test]
    fn cer_enxerga_o_acento_que_o_wer_ve_como_palavra_inteira() {
        let n = norm();
        let w = wer("a análise está pronta", "a analise esta pronta", &n);
        let c = cer("a análise está pronta", "a analise esta pronta", &n);
        assert_eq!(w.errors(), 2, "duas palavras erradas");
        assert_eq!(c.errors(), 2, "dois caracteres errados");
        assert!(c.rate < w.rate, "CER é mais fino: {} vs {}", c.rate, w.rate);
    }

    #[test]
    fn referencia_vazia() {
        assert_eq!(wer("", "", &norm()).rate, 0.0);
        assert_eq!(wer("", "alguma coisa", &norm()).rate, 1.0);
        let e = wer("uma coisa", "", &norm());
        assert_eq!(e.deletions, 2);
        assert_eq!(e.rate, 1.0);
    }

    #[test]
    fn normalizacao_sem_acento_e_opcional() {
        let sem = Normalization {
            strip_accents: true,
            ..Normalization::default()
        };
        assert_eq!(wer("análise", "analise", &sem).errors(), 0);
        assert_eq!(wer("análise", "analise", &norm()).errors(), 1);
    }

    fn t(a: f32, b: f32, spk: u32) -> SpeakerTurn {
        SpeakerTurn {
            start_secs: a,
            end_secs: b,
            speaker: spk,
        }
    }

    #[test]
    fn der_zero_quando_so_os_rotulos_trocam_de_nome() {
        // Mesma segmentação, ids permutados: o DER tem que enxergar 0.
        let r = [t(0.0, 5.0, 0), t(5.0, 10.0, 1)];
        let h = [t(0.0, 5.0, 7), t(5.0, 10.0, 3)];
        let d = der(&r, &h);
        assert!(d.rate < 0.01, "{d:?}");
    }

    #[test]
    fn der_conta_confusao_omissao_e_falso_alarme() {
        let r = [t(0.0, 10.0, 0), t(10.0, 20.0, 1)];
        // A hipótese começa 2 s atrasada (omissão), troca de falante 2 s
        // depois da hora (confusão) e inventa 3 s de fala depois do fim
        // (falso alarme). O casamento ótimo aqui é o óbvio, 0→0 e 1→1.
        let h = [t(2.0, 12.0, 0), t(12.0, 20.0, 1), t(20.0, 23.0, 1)];
        let d = der(&r, &h);
        assert!((d.missed_secs - 2.0).abs() < 0.05, "{d:?}");
        assert!((d.false_alarm_secs - 3.0).abs() < 0.05, "{d:?}");
        assert!((d.confusion_secs - 2.0).abs() < 0.05, "{d:?}");
        assert!((d.reference_secs - 20.0).abs() < 0.05, "{d:?}");
        assert!((d.rate - 0.35).abs() < 0.01, "{d:?}");
    }

    #[test]
    fn der_vazio_nao_explode() {
        assert_eq!(der(&[], &[]).rate, 0.0);
    }

    #[test]
    fn relatorio_vira_json() {
        let r = PipelineReport {
            config: "teste".into(),
            model: "ggml-small.bin".into(),
            device: device().into(),
            dtw: false,
            audio_secs: 10.0,
            realtime_factor: 0.5,
            timings: StageTimings::default(),
            params: RunParams {
                profile: TranscriptionProfile::MeetingFinal,
                lang: "pt".into(),
                decode: DecodeConfig::meeting_final(),
                vad: Some(VadOptions::default()),
                windows: WindowOptions::default(),
                align: AlignOptions::default(),
                context_chars: 400,
            },
            vad: VadStats::default(),
            asr: AsrStats::default(),
            diarization: None,
            speakers: SpeakerMetrics {
                speakers: 0,
                switches: 0,
                median_turn_secs: 0.0,
                very_short_turns: 0,
                unassigned: 0,
            },
        };
        let json = r.to_json();
        assert!(json.contains("\"beam_size\": 5"), "{json}");
        let volta: PipelineReport = serde_json::from_str(&json).expect("json válido");
        assert_eq!(volta, r);
    }
}
