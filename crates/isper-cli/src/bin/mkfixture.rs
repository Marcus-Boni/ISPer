//! `mkfixture` — gera o corpus de regressão do pipeline a partir de um roteiro.
//!
//! ```text
//! cargo run --release -p isper-cli --bin mkfixture -- fixtures/reuniao-sintetica.txt
//! ```
//!
//! Sai um `.wav` de 16 kHz mono, a transcrição de referência (`.ref.txt`) e os
//! turnos de falante de referência (`.turns.tsv`). Como o roteiro é a fonte, o
//! áudio não precisa ser versionado — e a verdade de referência não é chute:
//! é o que mandamos sintetizar.
//!
//! As vozes saem do SAPI do Windows. Como esta máquina só tem uma voz em
//! pt-BR, os falantes são separados por **perturbação de velocidade**
//! (1,00 · 0,92 · 1,08), o mesmo recurso que o treino de reconhecimento de
//! locutor usa para fabricar falantes distintos a partir de um só.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use isper_core::WHISPER_SAMPLE_RATE;

/// Uma fala do roteiro.
struct Line {
    pause_secs: f32,
    speaker: String,
    text: String,
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let script = PathBuf::from(
        args.first()
            .map(String::as_str)
            .unwrap_or("fixtures/reuniao-sintetica.txt"),
    );
    let voice = args
        .iter()
        .position(|a| a == "--voice")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "Microsoft Maria Desktop".into());

    let lines = parse_script(&script)?;
    anyhow::ensure!(!lines.is_empty(), "roteiro vazio: {}", script.display());
    let mut speakers: Vec<String> = Vec::new();
    for l in &lines {
        if !speakers.contains(&l.speaker) {
            speakers.push(l.speaker.clone());
        }
    }
    println!(
        "{} fala(s), {} falante(s): {}",
        lines.len(),
        speakers.len(),
        speakers.join(", ")
    );

    let tmp = std::env::temp_dir().join("isper-mkfixture");
    std::fs::create_dir_all(&tmp)?;

    let mut audio: Vec<f32> = Vec::new();
    let mut turns: Vec<(f32, f32, String)> = Vec::new();
    let mut reference = String::new();

    for (i, line) in lines.iter().enumerate() {
        pad(&mut audio, line.pause_secs);
        let clip = tmp.join(format!("clip-{i:03}.wav"));
        synth(&voice, &line.text, &clip)?;
        let raw = isper_core::audio::load_wav(&clip)?;
        let mono = raw.into_whisper_input()?;
        let idx = speakers
            .iter()
            .position(|s| *s == line.speaker)
            .unwrap_or_default();
        let perturbed = resample_by(&mono, speed_factor(idx));

        let start = audio.len() as f32 / WHISPER_SAMPLE_RATE as f32;
        audio.extend_from_slice(&perturbed);
        let end = audio.len() as f32 / WHISPER_SAMPLE_RATE as f32;
        turns.push((start, end, line.speaker.clone()));
        if !reference.is_empty() {
            reference.push(' ');
        }
        reference.push_str(line.text.trim());
        print!("\r  sintetizando {}/{}", i + 1, lines.len());
        let _ = std::io::stdout().flush();
    }
    println!();
    // Um respiro no fim: sem ele, a última palavra encosta na borda do arquivo.
    pad(&mut audio, 1.0);

    let stem = script.with_extension("");
    let wav = PathBuf::from(format!("{}-16k.wav", stem.display()));
    write_wav(&wav, &audio)?;
    std::fs::write(
        PathBuf::from(format!("{}.ref.txt", stem.display())),
        &reference,
    )?;
    let tsv: String = turns
        .iter()
        .map(|(a, b, s)| format!("{a:.3}\t{b:.3}\t{s}\n"))
        .collect();
    std::fs::write(
        PathBuf::from(format!("{}.turns.tsv", stem.display())),
        format!("# início\tfim\tfalante — verdade de referência para o DER\n{tsv}"),
    )?;

    let _ = std::fs::remove_dir_all(&tmp);
    println!(
        "pronto: {} ({:.1}s), {}.ref.txt, {}.turns.tsv",
        wav.display(),
        audio.len() as f32 / WHISPER_SAMPLE_RATE as f32,
        stem.display(),
        stem.display()
    );
    Ok(())
}

/// Fator de velocidade por falante. 1,00 · 0,92 · 1,08 · 0,86 · 1,14 …
fn speed_factor(idx: usize) -> f32 {
    const FATORES: [f32; 5] = [1.0, 0.92, 1.08, 0.86, 1.14];
    FATORES[idx % FATORES.len()]
}

fn parse_script(path: &Path) -> anyhow::Result<Vec<Line>> {
    let texto = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("falha ao ler {}: {e}", path.display()))?;
    let mut out = Vec::new();
    for (n, linha) in texto.lines().enumerate() {
        let linha = linha.trim();
        if linha.is_empty() || linha.starts_with('#') {
            continue;
        }
        let campos: Vec<&str> = linha.splitn(3, '|').collect();
        anyhow::ensure!(
            campos.len() == 3,
            "{}:{}: esperava <pausa>|<falante>|<texto>",
            path.display(),
            n + 1
        );
        out.push(Line {
            pause_secs: campos[0].trim().parse().map_err(|e| {
                anyhow::anyhow!("{}:{}: pausa inválida ({e})", path.display(), n + 1)
            })?,
            speaker: campos[1].trim().to_string(),
            text: campos[2].trim().to_string(),
        });
    }
    Ok(out)
}

fn pad(audio: &mut Vec<f32>, secs: f32) {
    let n = (secs.max(0.0) * WHISPER_SAMPLE_RATE as f32) as usize;
    audio.extend(std::iter::repeat_n(0.0f32, n));
}

/// Sintetiza um texto com o SAPI do Windows, via PowerShell.
fn synth(voice: &str, text: &str, dest: &Path) -> anyhow::Result<()> {
    // Aspas simples dobradas: é como PowerShell escapa dentro de uma string
    // literal, e o texto do roteiro é dado, não código.
    let esc = |s: &str| s.replace('\'', "''");
    let script = format!(
        "Add-Type -AssemblyName System.Speech; \
         $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
         $s.SelectVoice('{}'); \
         $f = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(16000, \
              [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, \
              [System.Speech.AudioFormat.AudioChannel]::Mono); \
         $s.SetOutputToWaveFile('{}', $f); \
         $s.Speak('{}'); \
         $s.Dispose()",
        esc(voice),
        esc(&dest.to_string_lossy()),
        esc(text)
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|e| anyhow::anyhow!("não consegui chamar o PowerShell: {e}"))?;
    anyhow::ensure!(
        out.status.success() && dest.exists(),
        "síntese falhou: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(())
}

/// Reamostra por interpolação linear — muda altura e formantes junto com a
/// duração, que é exatamente o efeito desejado para fabricar outro falante.
fn resample_by(samples: &[f32], factor: f32) -> Vec<f32> {
    if (factor - 1.0).abs() < 1e-6 || samples.is_empty() {
        return samples.to_vec();
    }
    let n = (samples.len() as f32 / factor) as usize;
    (0..n)
        .map(|i| {
            let pos = i as f32 * factor;
            let a = pos as usize;
            let frac = pos - a as f32;
            let s0 = samples.get(a).copied().unwrap_or(0.0);
            let s1 = samples.get(a + 1).copied().unwrap_or(s0);
            s0 + (s1 - s0) * frac
        })
        .collect()
}

fn write_wav(path: &Path, samples: &[f32]) -> anyhow::Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: WHISPER_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec)?;
    for s in samples {
        w.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
    }
    w.finalize()?;
    Ok(())
}
