//! Captura de microfone (cpal), leitura de WAV (hound) e resample (rubato).

use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

use crate::{IsperError, Result, WHISPER_SAMPLE_RATE};

/// Um microfone em captura compartilhada entrega pacotes de ~10 ms sem parar,
/// mesmo em silêncio absoluto. Ficar este tempo sem NENHUM pacote significa que
/// o stream morreu: fone desconectado, dispositivo invalidado depois de uma
/// suspensão ou driver travado — e não que a pessoa parou de falar.
pub const MIC_STALL: Duration = Duration::from_secs(2);
/// Antes do primeiro pacote a tolerância é maior: fones Bluetooth levam alguns
/// segundos trocando de perfil até começarem a entregar áudio.
pub const MIC_FIRST_PACKET: Duration = Duration::from_secs(6);

/// Traduz os erros mais comuns do WASAPI/cpal (código HRESULT ou frase em
/// inglês) numa dica em português sobre o que fazer. `None` quando não há
/// uma causa conhecida — aí a mensagem original é o que temos.
pub fn explain_error(message: &str) -> Option<&'static str> {
    let m = message.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| m.contains(n));
    if has(&["8889000a", "device_in_use", "being used by another process"]) {
        Some(
            "outro aplicativo está usando o dispositivo de áudio em modo exclusivo — feche-o ou \
             desmarque \"Permitir que aplicativos assumam o controle exclusivo\" nas propriedades \
             do dispositivo, em Som",
        )
    } else if has(&[
        "88890004",
        "device_invalidated",
        "no longer available",
        "unplugged",
        "88890026", // AUDCLNT_E_RESOURCES_INVALIDATED (depois de suspensão)
    ]) {
        Some(
            "o dispositivo de áudio foi desconectado ou mudou (fone removido, suspensão) — \
             reconecte-o ou escolha outro microfone em Configurações",
        )
    } else if has(&["88890008", "unsupported_format", "format is not supported"]) {
        Some(
            "o formato do dispositivo não é aceito — em Som → propriedades do dispositivo → \
             Avançado, escolha 16 ou 24 bits a 44,1 ou 48 kHz",
        )
    } else if has(&[
        "80070490",
        "e_notfound",
        "no input device",
        "nenhum dispositivo",
    ]) {
        Some("nenhum dispositivo de áudio ativo — conecte um microfone ou habilite-o em Som")
    } else {
        None
    }
}

/// A mensagem original seguida da dica de [`explain_error`], quando houver.
pub fn describe_error(message: &str) -> String {
    match explain_error(message) {
        Some(hint) => format!("{message} — {hint}"),
        None => message.to_string(),
    }
}

/// `IsperError::Audio` com a dica embutida — para todo erro de dispositivo.
pub(crate) fn audio_err(e: impl std::fmt::Display) -> IsperError {
    IsperError::Audio(describe_error(&e.to_string()))
}

/// Áudio cru como veio da fonte: amostras intercaladas na taxa original.
/// Se stereo, o layout é [esq, dir, esq, dir, ...].
#[derive(Debug, Clone)]
pub struct RawAudio {
    /// Amostras em f32 normalizado (`-1.0..=1.0`), intercaladas por canal.
    pub samples: Vec<f32>,
    /// Taxa de amostragem original, em Hz.
    pub sample_rate: u32,
    /// Número de canais intercalados em `samples`.
    pub channels: u16,
}

impl RawAudio {
    /// Duração do áudio, em segundos (0 se não houver canais).
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
    // Underrun/overrun no início do loopback é transitório e inofensivo; um
    // dispositivo que sumiu aparece aqui com a dica — e quem captura percebe
    // pela falta de pacotes ([`MIC_STALL`]) e reabre.
    tracing::warn!(
        "aviso no stream de áudio: {}",
        describe_error(&e.to_string())
    );
}

/// Grava do microfone padrão por `duration` (bloqueante — usado pela CLI).
pub fn record(duration: Duration) -> Result<RawAudio> {
    let (tx, rx) = crossbeam_channel::unbounded::<Vec<f32>>();
    let (stream, sample_rate, channels) = open_input_stream(tx)?;
    stream.play().map_err(audio_err)?;
    std::thread::sleep(duration);
    drop(stream); // encerra a captura; o lado `tx` do canal morre junto

    let samples: Vec<f32> = rx.try_iter().flatten().collect();
    Ok(RawAudio {
        samples,
        sample_rate,
        channels,
    })
}

/// Duração de cada leitura do medidor de nível (~20 por segundo).
pub const METER_WINDOW: Duration = Duration::from_millis(50);

/// Como terminou uma medição de [`monitor_input`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorEnd {
    /// Quem mediu pediu para parar.
    Stopped,
    /// O tempo máximo passou.
    Timeout,
    /// O microfone abriu mas não entregou áudio (desconectado, bloqueado em
    /// Privacidade → Microfone ou em uso exclusivo por outro programa).
    NoAudio,
}

/// Mede o nível do microfone ao vivo, sem gravar nada — o teste de microfone
/// da primeira execução. Abre `device` (ou o padrão, se `None` ou se ele não
/// existir mais) e chama `on_level` com o RMS de cada [`METER_WINDOW`] até
/// `stop` virar `true`, `max` passar ou o microfone parar de entregar áudio.
/// Bloqueante: o `cpal::Stream` nasce e morre nesta thread (não é `Send`).
pub fn monitor_input(
    device: Option<&str>,
    max: Duration,
    stop: &std::sync::atomic::AtomicBool,
    mut on_level: impl FnMut(f32),
) -> Result<MonitorEnd> {
    use std::sync::atomic::Ordering;
    use std::time::Instant;

    let (tx, rx) = crossbeam_channel::unbounded::<Vec<f32>>();
    let (stream, sample_rate, channels) = open_input_stream_on(device, tx)?;
    stream.play().map_err(audio_err)?;
    let per_window =
        (sample_rate as f32 * METER_WINDOW.as_secs_f32()) as usize * channels.max(1) as usize;
    let mut window = LevelWindow::new(per_window);
    let mut levels = Vec::new();
    let started = Instant::now();
    let mut last_packet: Option<Instant> = None;
    // Espera curta: o pedido de parar é atendido em até ~100 ms mesmo sem áudio.
    let poll = Duration::from_millis(100);
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(MonitorEnd::Stopped);
        }
        let now = Instant::now();
        if now.duration_since(started) >= max {
            return Ok(MonitorEnd::Timeout);
        }
        let silent_for = now.duration_since(last_packet.unwrap_or(started));
        let tolerance = if last_packet.is_some() {
            MIC_STALL
        } else {
            MIC_FIRST_PACKET
        };
        if silent_for >= tolerance {
            return Ok(MonitorEnd::NoAudio);
        }
        if let Ok(chunk) = rx.recv_timeout(poll) {
            last_packet = Some(Instant::now());
            levels.clear();
            window.push(&chunk, &mut levels);
            levels.iter().for_each(|&l| on_level(l));
        }
    }
}

/// Acumula amostras e fecha uma janela de RMS a cada `size` amostras — a
/// parte pura do medidor de nível, separada da captura para ter teste.
pub(crate) struct LevelWindow {
    size: usize,
    sum: f32,
    count: usize,
}

impl LevelWindow {
    pub(crate) fn new(size: usize) -> Self {
        Self {
            size: size.max(1),
            sum: 0.0,
            count: 0,
        }
    }

    /// Soma `chunk` e acrescenta a `out` o RMS de cada janela que fechou.
    pub(crate) fn push(&mut self, chunk: &[f32], out: &mut Vec<f32>) {
        for s in chunk {
            self.sum += s * s;
            self.count += 1;
            if self.count == self.size {
                out.push((self.sum / self.size as f32).sqrt());
                self.sum = 0.0;
                self.count = 0;
            }
        }
    }
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
    open_input_stream_on(None, tx)
}

/// Nomes dos dispositivos de entrada disponíveis (tela de Configurações).
/// O padrão do sistema é representado por `None` no resto do código.
pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    let mut names: Vec<String> = host
        .input_devices()
        .map(|devices| {
            devices
                .filter_map(|d| d.description().ok().map(|x| x.name().to_string()))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names.dedup();
    names
}

/// Como [`open_input_stream`], mas num microfone específico (pelo nome que
/// [`list_input_devices`] devolve). Se ele não existir mais — fone
/// desconectado, por exemplo — cai para o padrão do sistema com aviso, em vez
/// de falhar a gravação.
pub(crate) fn open_input_stream_on(
    device_name: Option<&str>,
    tx: crossbeam_channel::Sender<Vec<f32>>,
) -> Result<(cpal::Stream, u32, u16)> {
    let host = cpal::default_host();
    let wanted = device_name.map(str::trim).filter(|n| !n.is_empty());
    let chosen = wanted.and_then(|name| {
        host.input_devices().ok().and_then(|mut devices| {
            devices.find(|d| d.description().map(|x| x.name() == name).unwrap_or(false))
        })
    });
    if wanted.is_some() && chosen.is_none() {
        tracing::warn!(
            "microfone '{}' não encontrado — usando o padrão do sistema",
            wanted.unwrap_or_default()
        );
    }
    let device = chosen
        .or_else(|| host.default_input_device())
        .ok_or(IsperError::NoInputDevice)?;
    let config = device.default_input_config().map_err(audio_err)?;

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
                stream_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let _ = tx.send(data.to_vec());
                },
                stream_err,
                None,
            )
            .map_err(audio_err)?,
        cpal::SampleFormat::I16 => device
            .build_input_stream(
                stream_config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let _ = tx.send(data.iter().map(|s| *s as f32 / i16::MAX as f32).collect());
                },
                stream_err,
                None,
            )
            .map_err(audio_err)?,
        cpal::SampleFormat::U16 => device
            .build_input_stream(
                stream_config,
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
            .map_err(audio_err)?,
        other => {
            return Err(IsperError::Audio(format!(
                "formato de amostra não suportado: {other:?}"
            )));
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
    let mut resampler = Resampler16k::new(from_rate)?;
    let mut out = Vec::with_capacity(resampler.expected_len(mono.len()));
    resampler.push(mono, &mut out)?;
    resampler.finish(&mut out)?;
    Ok(out)
}

/// Tamanho do bloco que o filtro processa de cada vez.
const RESAMPLE_CHUNK: usize = 1024;

/// Reamostrador para 16 kHz que recebe o áudio aos pedaços.
///
/// É o mesmo filtro do [`resample_to_16k`] — que, aliás, é feito com ele —,
/// para quem não tem o áudio inteiro na memória: um arquivo de duas horas
/// decodificado em 48 kHz estéreo ocuparia 2,8 GB antes de ser convertido; em
/// fluxo, só o resultado em 16 kHz fica guardado. Entregar o áudio inteiro de
/// uma vez ou em pedaços de qualquer tamanho dá exatamente a mesma saída.
pub struct Resampler16k {
    /// `None` quando o áudio já está em 16 kHz: passa direto.
    inner: Option<SincFixedIn<f32>>,
    /// O que ainda não completou um bloco do filtro.
    pending: Vec<f32>,
    ratio: f64,
}

impl Resampler16k {
    /// Um reamostrador de `from_rate` Hz para 16 kHz.
    pub fn new(from_rate: u32) -> Result<Self> {
        if from_rate == 0 {
            return Err(IsperError::Resample("taxa de amostragem zero".into()));
        }
        if from_rate == WHISPER_SAMPLE_RATE {
            return Ok(Self {
                inner: None,
                pending: Vec::new(),
                ratio: 1.0,
            });
        }
        let params = SincInterpolationParameters {
            sinc_len: 256,
            f_cutoff: 0.95,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: 256,
            window: WindowFunction::BlackmanHarris2,
        };
        let ratio = WHISPER_SAMPLE_RATE as f64 / from_rate as f64;
        let inner = SincFixedIn::<f32>::new(ratio, 2.0, params, RESAMPLE_CHUNK, 1)
            .map_err(|e| IsperError::Resample(e.to_string()))?;
        Ok(Self {
            inner: Some(inner),
            pending: Vec::with_capacity(RESAMPLE_CHUNK * 2),
            ratio,
        })
    }

    /// Quantas amostras de 16 kHz saem, mais ou menos, de `input_len`
    /// amostras na taxa original — para reservar a memória de uma vez.
    pub fn expected_len(&self, input_len: usize) -> usize {
        (input_len as f64 * self.ratio) as usize + RESAMPLE_CHUNK
    }

    /// Entrega mais áudio mono na taxa original; o que já dá para converter
    /// vai para o fim de `out`.
    pub fn push(&mut self, mono: &[f32], out: &mut Vec<f32>) -> Result<()> {
        let Some(filter) = self.inner.as_mut() else {
            out.extend_from_slice(mono);
            return Ok(());
        };
        self.pending.extend_from_slice(mono);
        let mut pos = 0;
        while pos + RESAMPLE_CHUNK <= self.pending.len() {
            let chunk_out = filter
                .process(&[&self.pending[pos..pos + RESAMPLE_CHUNK]], None)
                .map_err(|e| IsperError::Resample(e.to_string()))?;
            out.extend_from_slice(&chunk_out[0]);
            pos += RESAMPLE_CHUNK;
        }
        self.pending.drain(..pos);
        Ok(())
    }

    /// Fecha: converte o último pedaço (menor que um bloco) e drena o atraso
    /// interno do filtro.
    pub fn finish(mut self, out: &mut Vec<f32>) -> Result<()> {
        let Some(mut filter) = self.inner.take() else {
            return Ok(());
        };
        let tail = [self.pending.as_slice()];
        let tail_in: Option<&[&[f32]]> = if self.pending.is_empty() {
            None
        } else {
            Some(&tail[..])
        };
        let chunk_out = filter
            .process_partial(tail_in, None)
            .map_err(|e| IsperError::Resample(e.to_string()))?;
        out.extend_from_slice(&chunk_out[0]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Um tom de `hz` em `rate` Hz, `secs` segundos.
    fn tom(hz: f32, rate: u32, secs: f32) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize)
            .map(|i| (i as f32 * hz * std::f32::consts::TAU / rate as f32).sin() * 0.5)
            .collect()
    }

    #[test]
    fn reamostrar_aos_pedacos_da_exatamente_o_mesmo_que_de_uma_vez() {
        let entrada = tom(440.0, 44_100, 1.3);
        let de_uma_vez = resample_to_16k(&entrada, 44_100).unwrap();
        // Pedaços de tamanhos que não casam com o bloco do filtro.
        let mut r = Resampler16k::new(44_100).unwrap();
        let mut aos_pedacos = Vec::new();
        let mut resto = entrada.as_slice();
        for n in [1usize, 777, 5000, 3, 20_000] {
            let (a, b) = resto.split_at(n.min(resto.len()));
            r.push(a, &mut aos_pedacos).unwrap();
            resto = b;
        }
        r.push(resto, &mut aos_pedacos).unwrap();
        r.finish(&mut aos_pedacos).unwrap();
        assert_eq!(de_uma_vez, aos_pedacos);
        // 1,3 s em 16 kHz, com a folga do filtro.
        let esperado = 1.3 * 16_000.0;
        assert!(
            (aos_pedacos.len() as f32 - esperado).abs() < 1500.0,
            "{} amostras para ~{esperado}",
            aos_pedacos.len()
        );
    }

    #[test]
    fn em_16k_o_reamostrador_so_repassa() {
        let entrada = tom(300.0, 16_000, 0.2);
        let mut r = Resampler16k::new(16_000).unwrap();
        let mut out = Vec::new();
        r.push(&entrada, &mut out).unwrap();
        r.finish(&mut out).unwrap();
        assert_eq!(out, entrada);
        assert!(
            Resampler16k::new(0).is_err(),
            "taxa zero é erro, não divisão por zero"
        );
    }

    #[test]
    fn medidor_fecha_uma_janela_a_cada_n_amostras_mesmo_entre_pacotes() {
        let mut w = LevelWindow::new(4);
        let mut out = Vec::new();
        // 3 + 3 amostras: a janela fecha no meio do segundo pacote.
        w.push(&[0.5, -0.5, 0.5], &mut out);
        assert!(out.is_empty(), "janela incompleta não sai");
        w.push(&[-0.5, 0.0, 0.0], &mut out);
        assert_eq!(out.len(), 1);
        assert!((out[0] - 0.5).abs() < 1e-6, "RMS de ±0,5 é 0,5: {}", out[0]);
        // Silêncio puro: RMS zero; as duas amostras que sobraram contam na próxima.
        w.push(&[0.0, 0.0], &mut out);
        assert_eq!(out.len(), 2);
        assert!(out[1].abs() < 1e-9);
        // Tamanho zero não trava nem divide por zero.
        let mut z = LevelWindow::new(0);
        z.push(&[1.0], &mut out);
        assert!((out[2] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn erros_de_dispositivo_ganham_dica_em_portugues() {
        // Códigos HRESULT do WASAPI, como o crate `wasapi`/`windows` os imprime.
        let in_use = describe_error("loopback (initialize captura): HRESULT 0x8889000A");
        assert!(in_use.contains("modo exclusivo"), "{in_use}");
        assert!(in_use.starts_with("loopback (initialize captura)"));
        assert!(describe_error("erro (0x88890004)").contains("desconectado"));
        assert!(describe_error("AUDCLNT_E_UNSUPPORTED_FORMAT").contains("44,1 ou 48 kHz"));
        // Frases do cpal.
        assert!(
            explain_error(
                "The requested device is no longer available. For example, it has been unplugged."
            )
            .is_some_and(|h| h.contains("desconectado"))
        );
        assert!(explain_error("The device is being used by another process").is_some());
        // Sem causa conhecida, a mensagem volta intacta.
        assert_eq!(explain_error("erro genérico 42"), None);
        assert_eq!(describe_error("erro genérico 42"), "erro genérico 42");
    }

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
        assert!(
            desvio < 0.05,
            "esperava ~16000 amostras, veio {}",
            out.len()
        );
    }
}
