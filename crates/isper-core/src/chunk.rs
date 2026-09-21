//! Onde cortar o áudio ao vivo — sem partir palavra.
//!
//! A transcrição AO VIVO não pode esperar o fim da reunião para rodar um VAD
//! sobre o arquivo inteiro (é o que faz [`crate::vad`], no passe final). Ela
//! precisa decidir, com o buffer na mão, se já dá para mandar um pedaço ao
//! Whisper.
//!
//! O jeito antigo era: ao chegar a 20 s, corte na janela de 100 ms de MENOR
//! energia dentro do último 1,5 s. O problema é que "menor energia" existe
//! sempre — inclusive no meio de uma palavra, na oclusão de um /t/ ou /p/.
//! Numa fala corrida, o corte caía dentro de "manual", o bloco seguinte
//! começava em "nual" e o modelo, sem contexto, escrevia "anual".
//!
//! Aqui o corte só vale se o ponto for silêncio DE VERDADE: energia bem
//! abaixo da do próprio bloco (critério relativo, porque o volume do loopback
//! varia) e abaixo de um piso absoluto. Sem silêncio à vista, o buffer
//! continua enchendo até um teto — e só aí cortamos onde for menos ruim,
//! avisando que foi um corte forçado.

use serde::{Deserialize, Serialize};

/// Regras do corte ao vivo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChunkOptions {
    /// A partir daqui já procuramos um lugar para cortar.
    pub target_secs: f32,
    /// Teto: chegando aqui, corta mesmo sem silêncio (latência tem limite).
    pub max_secs: f32,
    /// Quanto do fim do buffer entra na busca pelo silêncio.
    pub search_secs: f32,
    /// Tamanho da janela de energia, em milissegundos.
    pub window_ms: u32,
    /// O ponto candidato precisa ter RMS menor que esta fração do RMS do
    /// bloco todo. 0,15 = quinze por cento do volume médio.
    pub silence_ratio: f32,
    /// …e também abaixo deste piso absoluto, para o critério relativo não
    /// achar "silêncio" dentro de um bloco que é todo alto.
    pub silence_floor: f32,
}

impl Default for ChunkOptions {
    fn default() -> Self {
        Self {
            target_secs: 20.0,
            max_secs: 32.0,
            search_secs: 4.0,
            window_ms: 100,
            silence_ratio: 0.15,
            silence_floor: 0.02,
        }
    }
}

impl ChunkOptions {
    /// Perfil das legendas ao vivo: blocos curtos. O atraso de uma legenda é o
    /// tamanho do bloco mais a inferência, então 20 s de alvo eram 20 s de
    /// espera. O passe final refaz tudo com o VAD sobre o arquivo inteiro —
    /// o corte curto não custa qualidade na ata, só no ao vivo, onde chegar
    /// cedo vale mais.
    pub fn live() -> Self {
        Self {
            target_secs: 6.0,
            max_secs: 12.0,
            search_secs: 2.5,
            ..Self::default()
        }
    }

    /// Perfil de folga, usado quando o worker está atrasado: cada inferência
    /// tem um custo fixo, então blocos maiores custam menos por segundo de
    /// áudio e a fila volta a esvaziar.
    pub fn relaxed() -> Self {
        Self::default()
    }
}

/// Por que o corte aconteceu naquele ponto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutReason {
    /// Achamos silêncio de verdade: ninguém foi cortado no meio.
    Silence,
    /// Estourou o teto sem silêncio à vista — cortamos no ponto mais quieto
    /// que havia. É o único caso em que uma palavra pode ser partida, e ele
    /// é contado nas métricas.
    Forced,
}

/// Onde cortar (índice em AMOSTRAS INTERCALADAS, múltiplo de `channels`) e
/// por quê. `None` = ainda não é hora.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cut {
    pub at: usize,
    pub reason: CutReason,
}

/// Decide se o buffer já deve ser cortado, e onde.
pub fn plan_cut(
    buf: &[f32],
    sample_rate: u32,
    channels: usize,
    opts: &ChunkOptions,
) -> Option<Cut> {
    let ch = channels.max(1);
    let frames = buf.len() / ch;
    if frames == 0 || sample_rate == 0 {
        return None;
    }
    let secs = frames as f32 / sample_rate as f32;
    if secs < opts.target_secs {
        return None;
    }
    let win = ((sample_rate as f32 * opts.window_ms as f32 / 1000.0) as usize).max(1);
    let search = ((sample_rate as f32 * opts.search_secs) as usize).min(frames);
    if search < win * 2 {
        // Buffer curto demais para procurar: só o teto manda.
        return (secs >= opts.max_secs).then_some(Cut {
            at: buf.len(),
            reason: CutReason::Forced,
        });
    }

    let overall = rms(buf);
    let limite = (overall * opts.silence_ratio).min(opts.silence_floor);
    let start = frames - search;
    let mut best_frame = frames;
    let mut best_rms = f32::MAX;
    let mut f = start;
    while f + win <= frames {
        let r = rms_frames(buf, ch, f, win);
        if r < best_rms {
            best_rms = r;
            best_frame = f + win / 2;
        }
        f += win / 2; // passos de meia janela
    }

    if best_rms <= limite {
        return Some(Cut {
            at: best_frame * ch,
            reason: CutReason::Silence,
        });
    }
    // Sem silêncio: só corta se o teto obrigar, e no ponto mais quieto.
    (secs >= opts.max_secs).then_some(Cut {
        at: best_frame * ch,
        reason: CutReason::Forced,
    })
}

/// RMS de `win` frames a partir de `from`, misturando os canais.
fn rms_frames(buf: &[f32], ch: usize, from: usize, win: usize) -> f32 {
    let mut e = 0.0f32;
    for fr in from..from + win {
        let mut s = 0.0f32;
        for c in 0..ch {
            s += buf[fr * ch + c];
        }
        let m = s / ch as f32;
        e += m * m;
    }
    (e / win as f32).sqrt()
}

fn rms(buf: &[f32]) -> f32 {
    if buf.is_empty() {
        return 0.0;
    }
    (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 16_000;

    /// Fala sintética: senoide de 200 Hz com a amplitude pedida.
    fn fala(secs: f32, amp: f32) -> Vec<f32> {
        let n = (SR as f32 * secs) as usize;
        (0..n)
            .map(|i| (i as f32 * 200.0 * std::f32::consts::TAU / SR as f32).sin() * amp)
            .collect()
    }

    fn silencio(secs: f32) -> Vec<f32> {
        vec![0.0; (SR as f32 * secs) as usize]
    }

    #[test]
    fn nao_corta_antes_do_alvo() {
        let buf = fala(10.0, 0.3);
        assert_eq!(plan_cut(&buf, SR, 1, &ChunkOptions::default()), None);
    }

    #[test]
    fn corta_no_silencio_quando_existe_um() {
        // 21 s de fala, 0,5 s de silêncio, mais 1 s de fala.
        let mut buf = fala(21.0, 0.3);
        buf.extend(silencio(0.5));
        buf.extend(fala(1.0, 0.3));
        let cut = plan_cut(&buf, SR, 1, &ChunkOptions::default()).expect("deveria cortar");
        assert_eq!(cut.reason, CutReason::Silence);
        let at_secs = cut.at as f32 / SR as f32;
        assert!(
            (21.0..=21.5).contains(&at_secs),
            "cortou em {at_secs:.2}s, fora do silêncio"
        );
    }

    #[test]
    fn fala_corrida_nao_e_cortada_no_alvo() {
        // É o caso do "ma|nual": 25 s sem uma única pausa. Antes, o corte
        // saía assim mesmo; agora o buffer continua até o teto.
        let buf = fala(25.0, 0.3);
        assert_eq!(
            plan_cut(&buf, SR, 1, &ChunkOptions::default()),
            None,
            "não havia silêncio — não devia cortar"
        );
    }

    #[test]
    fn teto_forca_o_corte_e_avisa() {
        let buf = fala(35.0, 0.3);
        let cut = plan_cut(&buf, SR, 1, &ChunkOptions::default()).expect("teto");
        assert_eq!(cut.reason, CutReason::Forced);
        assert!(cut.at > 0 && cut.at <= buf.len());
    }

    #[test]
    fn corte_cai_em_fronteira_de_frame_no_stereo() {
        let mono = {
            let mut b = fala(21.0, 0.3);
            b.extend(silencio(0.5));
            b.extend(fala(1.0, 0.3));
            b
        };
        // Intercala o mesmo sinal nos dois canais.
        let stereo: Vec<f32> = mono.iter().flat_map(|s| [*s, *s]).collect();
        let cut = plan_cut(&stereo, SR, 2, &ChunkOptions::default()).expect("deveria cortar");
        assert_eq!(cut.at % 2, 0, "corte no meio de um frame");
        assert!(cut.at <= stereo.len());
        let at_secs = cut.at as f32 / 2.0 / SR as f32;
        assert!((21.0..=21.5).contains(&at_secs), "{at_secs:.2}s");
    }

    #[test]
    fn bloco_alto_nao_inventa_silencio_pelo_criterio_relativo() {
        // Sinal forte o tempo todo com um "vale" que é 10% do volume: o
        // critério relativo aceitaria, o piso absoluto não — e é aí que
        // palavra era partida.
        let mut buf = fala(21.0, 0.8);
        buf.extend(fala(0.3, 0.08)); // vale a 10% de 0,8
        buf.extend(fala(1.0, 0.8));
        let cut = plan_cut(&buf, SR, 1, &ChunkOptions::default());
        assert_eq!(cut, None, "0,08 de amplitude ainda é fala");
    }

    #[test]
    fn buffer_vazio_ou_taxa_zero_nao_quebram() {
        assert_eq!(plan_cut(&[], SR, 1, &ChunkOptions::default()), None);
        assert_eq!(
            plan_cut(&fala(30.0, 0.3), 0, 1, &ChunkOptions::default()),
            None
        );
    }

    // Opções compactas: os testes de propriedade rodam centenas de buffers, e
    // 20 s de áudio por caso seria minuto de CPU sem ganho de cobertura.
    #[test]
    fn perfil_ao_vivo_corta_no_silencio_bem_antes_do_padrao() {
        // 7 s de fala com 0,3 s de silêncio a 0,5 s do fim: o perfil ao vivo
        // (alvo 6 s) corta; o padrão (alvo 20 s) ainda espera.
        let mut buf = fala(7.0, 0.3);
        let frames = buf.len();
        let silence_start = frames - (SR as usize) / 2;
        for v in &mut buf[silence_start..silence_start + (SR as usize) * 3 / 10] {
            *v = 0.0;
        }
        let live = plan_cut(&buf, SR, 1, &ChunkOptions::live()).expect("perfil ao vivo corta");
        assert_eq!(live.reason, CutReason::Silence);
        assert!(live.at >= silence_start && live.at <= silence_start + (SR as usize) * 3 / 10);
        assert_eq!(plan_cut(&buf, SR, 1, &ChunkOptions::default()), None);
        assert!(ChunkOptions::live().max_secs < ChunkOptions::relaxed().target_secs);
    }

    fn curtas() -> ChunkOptions {
        ChunkOptions {
            target_secs: 2.0,
            max_secs: 4.0,
            search_secs: 1.5,
            ..ChunkOptions::default()
        }
    }

    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// O corte nunca passa do buffer e sempre cai numa fronteira de frame,
        /// dentro da janela de busca. Buffer abaixo do alvo não é cortado.
        #[test]
        fn corte_fica_no_buffer_e_em_fronteira_de_frame(
            samples in proptest::collection::vec(-1.0f32..1.0, 0..160_000),
            rate in prop_oneof![Just(8_000u32), Just(16_000), Just(48_000)],
            ch in 1usize..=2,
        ) {
            let opts = curtas();
            let Some(cut) = plan_cut(&samples, rate, ch, &opts) else {
                // Sem corte: ou o buffer é curto, ou não havia silêncio e o
                // teto ainda não estourou.
                let secs = (samples.len() / ch) as f32 / rate as f32;
                prop_assert!(secs < opts.max_secs);
                return Ok(());
            };
            prop_assert!(cut.at <= samples.len());
            prop_assert_eq!(cut.at % ch, 0);
            let frames = samples.len() / ch;
            let search = ((rate as f32 * opts.search_secs) as usize).min(frames);
            prop_assert!(cut.at / ch >= frames - search);
        }

        /// Um trecho de silêncio (300 ms) dentro da janela de busca é onde o
        /// corte cai, qualquer que seja o ruído em volta.
        #[test]
        fn corte_cai_no_silencio(
            rate in prop_oneof![Just(16_000u32), Just(48_000)],
            ch in 1usize..=2,
            secs in 2.5f32..3.5,
            // Onde o silêncio começa, contado do fim (dentro da busca).
            from_end_ms in 400u32..1_400,
            seed in any::<u64>(),
        ) {
            let frames = (secs * rate as f32) as usize;
            let silence_len = rate as usize * 3 / 10;
            let silence_start = frames - from_end_ms as usize * rate as usize / 1000;
            let mut state = seed | 1;
            let mut buf = Vec::with_capacity(frames * ch);
            for f in 0..frames {
                let v = if (silence_start..silence_start + silence_len).contains(&f) {
                    0.0
                } else {
                    // Ruído alto (módulo entre 0,2 e 1,0), sinal por xorshift.
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    let mag = 0.2 + (state % 800) as f32 / 1000.0;
                    if state & 2 == 0 { mag } else { -mag }
                };
                buf.extend(std::iter::repeat_n(v, ch));
            }
            let cut = plan_cut(&buf, rate, ch, &curtas()).expect("havia silêncio de sobra");
            prop_assert_eq!(cut.reason, CutReason::Silence);
            let cut_frame = cut.at / ch;
            prop_assert!(
                (silence_start..=silence_start + silence_len).contains(&cut_frame),
                "corte em {}, silêncio em {}..{}",
                cut_frame,
                silence_start,
                silence_start + silence_len
            );
        }
    }
}
