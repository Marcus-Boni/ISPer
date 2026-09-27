//! Calibra a conferência com a voz inteira da diarização
//! ([`isper_diarize::voice_similarities`], ADR 0005).
//!
//! Com os turnos de referência de um áudio (`início<TAB>fim<TAB>falante`),
//! parte a fala de cada falante em duas metades intercaladas e mede a
//! semelhança entre as metades (a MESMA voz) e entre falantes (vozes
//! DIFERENTES). O limiar [`isper_diarize::DEFAULT_SAME_VOICE`] tem de ficar
//! entre as duas faixas. Sem turnos, o áudio é de uma pessoa só, e a fala que
//! o agrupamento achou é partida do mesmo jeito.
//!
//! ```text
//! cargo run --release -p isper-cli --example conferir_vozes -- fixtures/reuniao-sintetica-16k.wav fixtures/reuniao-sintetica.turns.tsv
//! cargo run --release -p isper-cli --example conferir_vozes -- gravacao-de-uma-pessoa.opus
//! ```

use std::path::{Path, PathBuf};

use isper_diarize::SpeakerTurn;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let audio = PathBuf::from(
        args.next()
            .ok_or_else(|| anyhow::anyhow!("uso: conferir_vozes <áudio> [turnos.tsv]"))?,
    );
    let reference = args.next().map(PathBuf::from);

    let samples = isper_core::decode::decode_to_16k(&audio, None, None)?.samples_16k;
    let (_, embedding) = isper_diarize::model_paths_in(&isper_diarize::models_dir()?);

    let (names, turns) = match &reference {
        Some(tsv) => read_turns(tsv)?,
        None => {
            let opts = isper_diarize::DiarizeOptions {
                same_voice_similarity: 0.0,
                ..Default::default()
            };
            let out = isper_diarize::diarize_with(&samples, &opts)?;
            anyhow::ensure!(
                out.metrics.speakers == 1,
                "sem turnos de referência, o áudio precisa ser de uma pessoa só (o agrupamento achou {})",
                out.metrics.speakers
            );
            (vec!["a voz".to_string()], out.turns)
        }
    };

    // Cada falante vira dois: os turnos pares numa metade, os ímpares na outra
    // (só os que entram na impressão de voz, para as metades ficarem parelhas).
    let mut seen = vec![0u32; names.len()];
    let halves: Vec<SpeakerTurn> = turns
        .iter()
        .filter(|t| t.secs() >= isper_diarize::MIN_TURN_SECS)
        .map(|t| {
            let k = &mut seen[t.speaker as usize];
            *k += 1;
            SpeakerTurn {
                speaker: t.speaker * 2 + *k % 2,
                ..*t
            }
        })
        .collect();
    let secs = |group: u32| -> f32 {
        let s: f32 = halves
            .iter()
            .filter(|t| t.speaker == group)
            .map(SpeakerTurn::secs)
            .sum();
        s.min(isper_diarize::VOICE_SECS)
    };

    let pairs = isper_diarize::voice_similarities(&embedding, &samples, &halves, 4)?;
    let find = |a: u32, b: u32| {
        pairs
            .iter()
            .find(|(x, y, _)| (*x, *y) == (a.min(b), a.max(b)))
            .map(|(_, _, s)| *s)
    };
    println!("mesma voz (uma metade da fala × a outra):");
    for (i, name) in names.iter().enumerate() {
        let (a, b) = (i as u32 * 2, i as u32 * 2 + 1);
        match find(a, b) {
            Some(s) => println!("  {name:<10} {s:.2}  ({:.1} s × {:.1} s)", secs(a), secs(b)),
            None => println!("  {name:<10} —     (fala curta demais para a impressão)"),
        }
    }
    if names.len() > 1 {
        let whole = isper_diarize::voice_similarities(&embedding, &samples, &turns, 4)?;
        println!("vozes diferentes (voz inteira · entre metades):");
        for (a, b, s) in &whole {
            let cross: Vec<f32> = [(0, 0), (0, 1), (1, 0), (1, 1)]
                .iter()
                .filter_map(|(x, y)| find(a * 2 + x, b * 2 + y))
                .collect();
            let lo = cross.iter().copied().fold(f32::INFINITY, f32::min);
            let hi = cross.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            println!(
                "  {:<10} × {:<10} {s:.2}  · {lo:.2} a {hi:.2}",
                names[*a as usize], names[*b as usize]
            );
        }
    }
    println!(
        "limiar da conferência: {}",
        isper_diarize::DEFAULT_SAME_VOICE
    );
    Ok(())
}

/// Os turnos de referência, com os falantes numerados por ordem de aparição.
fn read_turns(path: &Path) -> anyhow::Result<(Vec<String>, Vec<SpeakerTurn>)> {
    let mut names: Vec<String> = Vec::new();
    let mut turns = Vec::new();
    for line in std::fs::read_to_string(path)?.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        anyhow::ensure!(cols.len() >= 3, "linha sem três colunas: {line}");
        let name = cols[2].trim().to_string();
        let speaker = match names.iter().position(|n| *n == name) {
            Some(i) => i,
            None => {
                names.push(name);
                names.len() - 1
            }
        } as u32;
        turns.push(SpeakerTurn {
            start: cols[0].trim().parse()?,
            end: cols[1].trim().parse()?,
            speaker,
        });
    }
    Ok((names, turns))
}
