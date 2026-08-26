//! Captura de microfone (cpal), leitura de WAV (hound) e resample (rubato).

use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

use crate::{IsperError, Result, WHISPER_SAMPLE_RATE};

/// Áudio cru como veio da fonte: amostras intercaladas na taxa original.
/// Se stereo, o layout é [esq, dir, esq, dir, ...].
#[derive(Debug, Clone)]
pub struct RawAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl RawAudio {
    pub fn duration_secs(&self) -> f32 {
        if self.channels == 0 {
            return 0.0;
        }
        self.samples.len() as f32 / self.channels as f32 / self.sample_rate as f32
    }

    /// Converte para o único formato que o Whisper aceita: 16 kHz mono f32.
    pub fn into_whisper_input(self) -> Result<Vec<f32>> {
        let mono = to_mono(&self.samples, self.channels);
        resample_to_16k(&mono, self.sample_rate)
    }

    /// Volume médio (RMS) — usado p/ pular blocos silenciosos sem gastar GPU.
    pub fn rms(&self) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }
        (self.samples.iter().map(|s| s * s).sum::<f32>() / self.samples.len() as f32).sqrt()
    }
}

fn stream_err(e: cpal::Error) {
    // Underrun/overrun no início do loopback é transitório e inofensivo.
    tracing::warn!("aviso no stream de áudio: {e}");
}

/// Grava do microfone padrão por `duration` (bloqueante — usado pela CLI).
pub fn record(duration: Duration) -> Result<RawAudio> {
    let (tx, rx) = crossbeam_channel::unbounded::<Vec<f32>>();
    let (stream, sample_rate, channels) = open_input_stream(tx)?;
    stream.play().map_err(|e| IsperError::Audio(e.to_string()))?;
    std::thread::sleep(duration);
    drop(stream); // encerra a captura; o lado `tx` do canal morre junto

    let samples: Vec<f32> = rx.try_iter().flatten().collect();
    Ok(RawAudio {
        samples,
        sample_rate,
        channels,
    })
}

/// Abre um stream de captura do microfone padrão. As amostras chegam pelo
/// canal `tx` já convertidas para f32 normalizado. O chamador dá `.play()`
/// e encerra a captura dropando o stream.
///
/// (O loopback do sistema vive no módulo [`crate::loopback`]: o caminho de
/// loopback do cpal estagna quando os eventos WASAPI param de chegar.)
///
/// O cpal entrega as amostras num callback que roda em OUTRA thread (a thread
/// de áudio do WASAPI). O canal leva os buffers de volta: é o jeito idiomático
/// em Rust de tirar dados de um callback sem compartilhar estado mutável —
/// o ownership de cada buffer é *transferido* pelo canal.
pub(crate) fn open_input_stream(
    tx: crossbeam_channel::Sender<Vec<f32>>,
) -> Result<(cpal::Stream, u32, u16)> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or(IsperError::NoInputDevice)?;
    let config = device
        .default_input_config()
        .map_err(|e| IsperError::Audio(e.to_string()))?;

    let sample_rate = config.sample_rate();
    let channels = config.channels();
    let device_name = device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_default();
    tracing::info!(device = %device_name, sample_rate, channels, "capturando mic");

    let stream_config: cpal::StreamConfig = config.config();

    // O formato das amostras depende do driver (f32 no WASAPI, i16 em outros).
    // Convertemos tudo para f32 normalizado [-1.0, 1.0] já no callback.
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device
            .build_input_stream(
                stream_config.clone(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let _ = tx.send(data.to_vec());
                },
                stream_err,
                None,
            )
            .map_err(|e| IsperError::Audio(e.to_string()))?,
        cpal::SampleFormat::I16 => device
            .build_input_stream(
                stream_config.clone(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let _ = tx.send(data.iter().map(|s| *s as f32 / i16::MAX as f32).collect());
                },
                stream_err,
                None,
            )
            .map_err(|e| IsperError::Audio(e.to_string()))?,
        cpal::SampleFormat::U16 => device
            .build_input_stream(
                stream_config.clone(),
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    let _ = tx.send(
                        data.iter()
                            .map(|s| (*s as f32 - 32_768.0) / 32_768.0)
                            .collect(),
                    );
                },
                stream_err,
                None,
            )
            .map_err(|e| IsperError::Audio(e.to_string()))?,
        other => {
            return Err(IsperError::Audio(format!(
                "formato de amostra não suportado: {other:?}"
            )))
        }
    };

    Ok((stream, sample_rate, channels))
}

/// Lê um WAV (int ou float, qualquer nº de canais) para RawAudio.
pub fn load_wav(path: &std::path::Path) -> Result<RawAudio> {
    let reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .collect::<std::result::Result<_, _>>()?,
        hound::SampleFormat::Int => {
            // Normaliza int de qualquer profundidade (16/24/32 bits) p/ [-1, 1].
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .into_samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max))
                .collect::<std::result::Result<_, _>>()?
        }
    };
    Ok(RawAudio {
        samples,
        sample_rate: spec.sample_rate,
        channels: spec.channels,
    })
}

/// Mistura canais intercalados para mono por média simples.
pub fn to_mono(interleaved: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    let ch = channels as usize;
    interleaved
        .chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

/// Reamostra mono f32 de `from_rate` para 16 kHz com filtro sinc (qualidade
/// alta — resample "ingênuo" por interpolação linear cria aliasing que
/// degrada a transcrição).
pub fn resample_to_16k(mono: &[f32], from_rate: u32) -> Result<Vec<f32>> {
    if from_rate == WHISPER_SAMPLE_RATE {
        return Ok(mono.to_vec());
    }

    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256,
        window: WindowFunction::BlackmanHarris2,
    };
    const CHUNK: usize = 1024;
    let ratio = WHISPER_SAMPLE_RATE as f64 / from_rate as f64;
    let mut resampler = SincFixedIn::<f32>::new(ratio, 2.0, params, CHUNK, 1)
        .map_err(|e| IsperError::Resample(e.to_string()))?;

    let mut out: Vec<f32> = Vec::with_capacity((mono.len() as f64 * ratio) as usize + CHUNK);
    let mut pos = 0;
    while pos + CHUNK <= mono.len() {
        let chunk_out = resampler
            .process(&[&mono[pos..pos + CHUNK]], None)
            .map_err(|e| IsperError::Resample(e.to_string()))?;
        out.extend_from_slice(&chunk_out[0]);
        pos += CHUNK;
    }
    // Último pedaço (menor que um chunk) + drenagem do atraso interno do filtro.
    let tail = &mono[pos..];
    let tail_in: Option<&[&[f32]]> = if tail.is_empty() { None } else { Some(&[tail]) };
    let chunk_out = resampler
        .process_partial(tail_in.map(|t| t), None)
        .map_err(|e| IsperError::Resample(e.to_string()))?;
    out.extend_from_slice(&chunk_out[0]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_mono_faz_media_dos_canais() {
        let stereo = vec![1.0, 0.0, 0.5, 0.5, -1.0, 1.0];
        assert_eq!(to_mono(&stereo, 2), vec![0.5, 0.5, 0.0]);
    }

    #[test]
    fn resample_reduz_na_proporcao_certa() {
        // 1 s de senoide de 440 Hz a 48 kHz deve virar ~16 mil amostras.
        let sr = 48_000u32;
        let mono: Vec<f32> = (0..sr)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / sr as f32).sin())
            .collect();
        let out = resample_to_16k(&mono, sr).unwrap();
        let expected = 16_000f32;
        let desvio = (out.len() as f32 - expected).abs() / expected;
        assert!(desvio < 0.05, "esperava ~16000 amostras, veio {}", out.len());
    }
}
