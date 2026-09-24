//! Arquivos de áudio de fora do ISPer — o gravador do celular, o Plaud, a
//! gravação de uma reunião do Teams — viram o formato do Whisper: 16 kHz,
//! mono, f32 (Fase 9.0).
//!
//! Tudo passa pelo [symphonia](https://github.com/pdeljanov/Symphonia), Rust
//! puro: MP3, M4A e MP4 (AAC), WAV, FLAC e OGG Vorbis. A leitura é em fluxo:
//! cada pacote decodificado vira mono e segue direto para o
//! [`Resampler16k`], então a memória é só a do resultado (≈ 230 MB por hora
//! de áudio) — decodificar tudo primeiro e converter depois custaria 1,4 GB
//! por hora num arquivo de celular em 48 kHz estéreo.
//!
//! Opus (o `.opus` das mensagens de voz) fica de fora por enquanto: o
//! symphonia ainda não tem o decodificador, e o libopus traria uma biblioteca
//! C para o build. O erro diz isso com todas as letras.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::codecs::audio::well_known::CODEC_ID_OPUS;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;

use crate::audio::{Resampler16k, to_mono};
use crate::{IsperError, Result, WHISPER_SAMPLE_RATE};

/// Extensões que o ISPer lê (minúsculas, sem o ponto).
pub const AUDIO_EXTENSIONS: [&str; 8] = ["mp3", "m4a", "mp4", "aac", "wav", "flac", "ogg", "oga"];

/// O arquivo tem uma extensão que o ISPer sabe ler?
pub fn is_supported(path: &Path) -> bool {
    extension(path).is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.as_str()))
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
}

/// O que sai de [`decode_to_16k`].
#[derive(Debug, Clone)]
pub struct DecodedAudio {
    /// 16 kHz, mono, f32 — pronto para o [`crate::pipeline`].
    pub samples_16k: Vec<f32>,
    /// Taxa de amostragem do arquivo, em Hz.
    pub source_rate: u32,
    /// Canais do arquivo (misturados em mono).
    pub source_channels: u16,
    /// Pacotes com defeito que foram pulados (0 num arquivo são).
    pub bad_packets: usize,
}

impl DecodedAudio {
    /// Duração do áudio, em segundos.
    pub fn duration_secs(&self) -> f32 {
        self.samples_16k.len() as f32 / WHISPER_SAMPLE_RATE as f32
    }
}

/// Progresso da leitura, de 0 a 1, pela posição no arquivo.
pub type DecodeProgress<'a> = &'a (dyn Fn(f32) + Send + Sync);

/// Lê um arquivo de áudio inteiro e o devolve em 16 kHz mono.
///
/// `progress` recebe a fração já lida (no máximo uma vez a cada 1%);
/// `cancel`, quando vira `true`, interrompe a leitura com
/// [`IsperError::Cancelled`].
pub fn decode_to_16k(
    path: &Path,
    progress: Option<DecodeProgress<'_>>,
    cancel: Option<&AtomicBool>,
) -> Result<DecodedAudio> {
    let ext = extension(path).unwrap_or_default();
    if !AUDIO_EXTENSIONS.contains(&ext.as_str()) {
        return Err(IsperError::Decode(unsupported_message(&ext)));
    }

    let file = File::open(path)?;
    let len = file.metadata()?.len();
    let read = Arc::new(AtomicU64::new(0));
    let source = CountingFile {
        file,
        len,
        read: Arc::clone(&read),
    };
    let mss = MediaSourceStream::new(Box::new(source), Default::default());
    let mut hint = Hint::new();
    hint.with_extension(&ext);
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| symphonia_err(e, &ext))?;

    // Um vídeo (a gravação de uma reunião, por exemplo) tem outras trilhas:
    // vale a de áudio padrão.
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| IsperError::Decode("o arquivo não tem nenhuma trilha de áudio".into()))?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| IsperError::Decode("a trilha de áudio não diz qual é o codec".into()))?
        .clone();
    if params.codec == CODEC_ID_OPUS {
        return Err(IsperError::Decode(unsupported_message("opus")));
    }
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| symphonia_err(e, &ext))?;

    let mut resampler: Option<Resampler16k> = None;
    let mut out: Vec<f32> = Vec::new();
    let mut interleaved: Vec<f32> = Vec::new();
    let mut source_rate = 0u32;
    let mut source_channels = 0u16;
    let mut packets = 0usize;
    let mut bad_packets = 0usize;
    let mut last_reported = -1.0f32;

    loop {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(IsperError::Cancelled);
        }
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            // Um OGG encadeado recomeça com outra trilha: a primeira basta.
            Err(SymphoniaError::ResetRequired) => break,
            // Arquivo truncado (a gravação parou no meio da cópia): vale o
            // que foi lido até ali.
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                tracing::warn!("o arquivo acaba no meio de um pacote — usando o que foi lido");
                break;
            }
            Err(e) => return Err(symphonia_err(e, &ext)),
        };
        if packet.track_id != track_id {
            continue;
        }
        packets += 1;
        let audio = match decoder.decode(&packet) {
            Ok(a) => a,
            // Pacote com defeito: pula e segue, como um player faria.
            Err(SymphoniaError::DecodeError(_)) | Err(SymphoniaError::IoError(_)) => {
                bad_packets += 1;
                continue;
            }
            Err(e) => return Err(symphonia_err(e, &ext)),
        };
        if audio.frames() == 0 {
            continue;
        }
        let spec = audio.spec();
        let rate = spec.rate();
        let channels = spec.channels().count().max(1) as u16;
        match resampler {
            None => {
                resampler = Some(Resampler16k::new(rate)?);
                source_rate = rate;
                source_channels = channels;
                out.reserve(expected_len(len, rate));
            }
            Some(_) if rate != source_rate => {
                return Err(IsperError::Decode(format!(
                    "a taxa de amostragem muda no meio do arquivo ({source_rate} → {rate} Hz)"
                )));
            }
            Some(_) => {}
        }
        interleaved.resize(audio.samples_interleaved(), 0.0);
        audio.copy_to_slice_interleaved(&mut interleaved[..]);
        let mono = to_mono(&interleaved, channels);
        if let Some(r) = resampler.as_mut() {
            r.push(&mono, &mut out)?;
        }

        if let Some(p) = progress
            && len > 0
        {
            let frac = (read.load(Ordering::Relaxed) as f32 / len as f32).min(1.0);
            if frac - last_reported >= 0.01 {
                p(frac);
                last_reported = frac;
            }
        }
    }

    if let Some(r) = resampler.take() {
        r.finish(&mut out)?;
    }
    if out.is_empty() {
        return Err(IsperError::Decode(
            if packets > 0 && bad_packets == packets {
                "nenhum pacote de áudio pôde ser decodificado (arquivo corrompido?)".into()
            } else {
                "o arquivo não tem áudio".into()
            },
        ));
    }
    if bad_packets > 0 {
        tracing::warn!(bad_packets, packets, "pacotes com defeito foram pulados");
    }
    if let Some(p) = progress {
        p(1.0);
    }
    Ok(DecodedAudio {
        samples_16k: out,
        source_rate,
        source_channels,
        bad_packets,
    })
}

/// Reserva inicial do resultado. Não dá para saber a duração antes de
/// decodificar sem confiar em metadados; a conta usa o tamanho do arquivo com
/// uma taxa de compressão típica e fica limitada a 2 h (o `Vec` cresce se
/// precisar).
fn expected_len(file_len: u64, _rate: u32) -> usize {
    // ~64 kbit/s é o típico de gravador de voz; 8 KB por segundo.
    let secs = (file_len / 8_000).min(2 * 3600);
    secs as usize * WHISPER_SAMPLE_RATE as usize
}

fn unsupported_message(ext: &str) -> String {
    let lista = "MP3, M4A, MP4, AAC, WAV, FLAC e OGG";
    match ext {
        "opus" => format!(
            "o áudio está em Opus, que o ISPer ainda não lê — converta para MP3 ou WAV (o ISPer lê {lista})"
        ),
        "" => format!("o arquivo não tem extensão (o ISPer lê {lista})"),
        other => format!("formato .{other} não suportado (o ISPer lê {lista})"),
    }
}

fn symphonia_err(e: SymphoniaError, ext: &str) -> IsperError {
    match e {
        SymphoniaError::IoError(io) => IsperError::Io(io),
        // Extensão conhecida e o leitor não reconhece o conteúdo: o arquivo
        // é que está errado (corrompido, ou outra coisa com a extensão
        // trocada) — dizer "formato não suportado" mandaria procurar outro
        // formato à toa.
        SymphoniaError::Unsupported(what) if AUDIO_EXTENSIONS.contains(&ext) => {
            IsperError::Decode(format!(
                "o arquivo não é um .{ext} válido — está corrompido ou tem a extensão errada ({what})"
            ))
        }
        SymphoniaError::Unsupported(what) => {
            IsperError::Decode(format!("{} ({what})", unsupported_message(ext)))
        }
        other => IsperError::Decode(other.to_string()),
    }
}

/// O arquivo, contando quantos bytes o symphonia já leu (para o progresso).
struct CountingFile {
    file: File,
    len: u64,
    read: Arc<AtomicU64>,
}

impl Read for CountingFile {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.file.read(buf)?;
        self.read.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

impl Seek for CountingFile {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let at = self.file.seek(pos)?;
        self.read.store(at, Ordering::Relaxed);
        Ok(at)
    }
}

impl MediaSource for CountingFile {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name)
    }

    /// A referência: o WAV original, em 16 kHz mono.
    fn referencia() -> Vec<f32> {
        crate::audio::load_wav(&fixture("fala-16k.wav"))
            .unwrap()
            .into_whisper_input()
            .unwrap()
    }

    /// Energia em janelas de 20 ms — compara a "forma" da fala sem depender
    /// de fase, atraso do codificador ou perdas do MP3/AAC.
    fn envelope(samples: &[f32]) -> Vec<f32> {
        samples
            .chunks(320)
            .map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt())
            .collect()
    }

    /// Maior correlação entre os envelopes com até ±10 janelas (200 ms) de
    /// deslocamento — o atraso de codificação do AAC e do MP3, que o
    /// decodificador de AAC não descarta (sem *gapless*).
    fn correlacao(a: &[f32], b: &[f32]) -> f32 {
        let mut melhor = f32::MIN;
        for shift in -10i32..=10 {
            let pares: Vec<(f32, f32)> = (0..a.len())
                .filter_map(|i| {
                    let j = i as i32 + shift;
                    (j >= 0 && (j as usize) < b.len()).then(|| (a[i], b[j as usize]))
                })
                .collect();
            let n = pares.len() as f32;
            let (ma, mb) = pares
                .iter()
                .fold((0.0, 0.0), |(x, y), (p, q)| (x + p / n, y + q / n));
            let (mut cov, mut va, mut vb) = (0.0f32, 0.0f32, 0.0f32);
            for (p, q) in &pares {
                cov += (p - ma) * (q - mb);
                va += (p - ma).powi(2);
                vb += (q - mb).powi(2);
            }
            melhor = melhor.max(cov / (va.sqrt() * vb.sqrt()).max(f32::EPSILON));
        }
        melhor
    }

    #[test]
    fn os_formatos_viram_o_mesmo_audio_em_16k() {
        let ref_env = envelope(&referencia());
        let ref_secs = 10.381;
        for (arquivo, rate, canais) in [
            ("formatos/fala-44k-stereo.mp3", 44_100, 2),
            ("formatos/fala-48k.m4a", 48_000, 1),
            // 8 kHz: o reamostrador sobe a taxa (gravador de chamada).
            ("formatos/fala-8k.flac", 8_000, 1),
            ("formatos/fala-22k.ogg", 22_050, 1),
            ("formatos/fala-32k.aac", 32_000, 1),
            ("formatos/fala-video.mp4", 44_100, 1),
            ("fala-48k-stereo.wav", 48_000, 2),
        ] {
            let d = decode_to_16k(&fixture(arquivo), None, None)
                .unwrap_or_else(|e| panic!("{arquivo}: {e}"));
            assert_eq!(d.source_rate, rate, "{arquivo}: taxa");
            assert_eq!(d.source_channels, canais, "{arquivo}: canais");
            assert_eq!(d.bad_packets, 0, "{arquivo}: pacotes com defeito");
            if !arquivo.starts_with("fala-48k") {
                // Mesma fala do WAV de referência: duração e forma batem.
                assert!(
                    (d.duration_secs() - ref_secs).abs() < 0.2,
                    "{arquivo}: {} s em vez de ~{ref_secs}",
                    d.duration_secs()
                );
                let c = correlacao(&ref_env, &envelope(&d.samples_16k));
                eprintln!("{arquivo}: {:.2} s, correlação {c:.3}", d.duration_secs());
                assert!(c > 0.95, "{arquivo}: correlação {c}");
            }
        }
    }

    #[test]
    fn progresso_chega_ao_fim_e_cancelar_interrompe() {
        let visto = Mutex::new(Vec::new());
        let cb = |f: f32| visto.lock().unwrap().push(f);
        decode_to_16k(&fixture("formatos/fala-8k.flac"), Some(&cb), None).unwrap();
        let v = visto.lock().unwrap();
        assert_eq!(v.last().copied(), Some(1.0), "termina em 100%: {v:?}");
        assert!(v.windows(2).all(|w| w[0] <= w[1]), "nunca volta: {v:?}");

        let cancelar = AtomicBool::new(true);
        let r = decode_to_16k(&fixture("formatos/fala-22k.ogg"), None, Some(&cancelar));
        assert!(matches!(r, Err(IsperError::Cancelled)), "{r:?}");
    }

    #[test]
    fn opus_e_extensoes_desconhecidas_dao_erro_que_explica() {
        let e = decode_to_16k(&fixture("formatos/fala-2s.opus"), None, None).unwrap_err();
        let msg = e.to_string();
        assert!(msg.contains("Opus") && msg.contains("MP3"), "{msg}");

        let e = decode_to_16k(Path::new("reuniao.wma"), None, None).unwrap_err();
        assert!(e.to_string().contains(".wma não suportado"), "{e}");

        assert!(is_supported(Path::new("Gravação 01.M4A")));
        assert!(is_supported(Path::new("reuniao.mp4")));
        assert!(!is_supported(Path::new("notas.txt")));
        assert!(!is_supported(Path::new("sem-extensao")));
    }

    #[test]
    fn arquivo_que_nao_e_audio_da_erro_em_vez_de_panico() {
        let dir = std::env::temp_dir().join(format!("isper-decode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let falso = dir.join("nao-e-audio.mp3");
        std::fs::write(
            &falso,
            b"isto nao e um mp3, e texto puro repetido ".repeat(200),
        )
        .unwrap();
        let msg = decode_to_16k(&falso, None, None)
            .expect_err("texto com extensão .mp3 não pode virar áudio")
            .to_string();
        assert!(msg.contains("não é um .mp3 válido"), "{msg}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
