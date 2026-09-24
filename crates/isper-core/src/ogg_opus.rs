//! Ogg/Opus ([RFC 7845](https://www.rfc-editor.org/rfc/rfc7845)): o formato
//! das gravações do celular (Fase 9.2) e o das mensagens de voz do WhatsApp.
//!
//! **Gravar** ([`OggOpusWriter`]) é pensado para sobreviver a qualquer
//! interrupção:
//!
//! - o Ogg é uma sequência de páginas independentes, cada uma com o próprio
//!   CRC — um arquivo cortado no meio continua legível até a última página
//!   inteira. Não há índice no fim do arquivo, que é o que torna ilegível um
//!   M4A interrompido;
//! - uma página fecha a cada segundo de áudio ([`PACKETS_PER_PAGE`]) e vai
//!   para o arquivo; a cada [`SYNC_EVERY_PAGES`] páginas o sistema é obrigado
//!   a gravar no disco (`sync_data`). Se o app morrer, perde-se no máximo a
//!   página em andamento (1 s); se o aparelho desligar sem aviso, alguns
//!   segundos.
//!
//! **Ler** ([`decode_to_16k`], [`duration_secs`]) aceita arquivo truncado:
//! vale o que foi gravado até a última página íntegra — é o que recupera uma
//! gravação depois de uma queda.

use std::fs::File;
use std::io::{BufReader, Seek};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use ogg::reading::PacketReader;
use ogg::writing::{PacketWriteEndInfo, PacketWriter};

use crate::audio::{Resampler16k, to_mono};
use crate::decode::{DecodeProgress, DecodedAudio};
use crate::{IsperError, Result};

/// A posição (granule) do Ogg/Opus conta amostras a 48 kHz, qualquer que seja
/// a taxa de entrada.
pub const GRANULE_RATE: u32 = 48_000;
/// 32 kbit/s mono: 14,4 MB por hora, e voz com folga para o Whisper (que
/// trabalha em 16 kHz). O corpus mede o efeito no WER (ver o ADR 0016).
pub const DEFAULT_BITRATE: i32 = 32_000;
/// Taxas que o Opus aceita na entrada.
pub const INPUT_RATES: [u32; 5] = [8_000, 12_000, 16_000, 24_000, 48_000];
/// Cada pacote tem 20 ms de áudio.
const FRAME_MS: u32 = 20;
/// Amostras a 48 kHz por pacote.
const FRAME_48K: u64 = (GRANULE_RATE * FRAME_MS / 1000) as u64;
/// 50 pacotes de 20 ms = uma página por segundo.
pub const PACKETS_PER_PAGE: u32 = 50;
/// Páginas entre um `sync_data` e outro (5 s).
pub const SYNC_EVERY_PAGES: u32 = 5;
/// Maior pacote que pedimos ao codificador (bem acima do que 32 kbit/s usa).
const MAX_PACKET: usize = 4_000;
/// Maior quadro que o decodificador devolve: 120 ms a 48 kHz, por canal.
const MAX_FRAME_48K: usize = 5_760;

fn opus_err(what: &str, e: opus::Error) -> IsperError {
    IsperError::Audio(format!("Opus ({what}): {e}"))
}

/// Grava áudio mono PCM 16 bits num arquivo Ogg/Opus, em páginas de 1 s.
pub struct OggOpusWriter {
    packets: PacketWriter<'static, File>,
    encoder: opus::Encoder,
    serial: u32,
    rate: u32,
    frame_len: usize,
    pending: Vec<i16>,
    pre_skip: u64,
    /// Amostras reais (na taxa de entrada) já recebidas — sem o enchimento do
    /// último pacote.
    samples_in: u64,
    /// Pacotes de áudio já escritos.
    frames: u64,
    packets_in_page: u32,
    pages_since_sync: u32,
}

impl OggOpusWriter {
    /// Cria o arquivo (nunca sobrescreve um que exista) e escreve os
    /// cabeçalhos. `rate` é a taxa do PCM que vai chegar ([`INPUT_RATES`]).
    pub fn create(path: &Path, rate: u32, bitrate: i32) -> Result<Self> {
        if !INPUT_RATES.contains(&rate) {
            return Err(IsperError::Audio(format!(
                "o Opus não aceita {rate} Hz (aceita 8, 12, 16, 24 ou 48 kHz)"
            )));
        }
        let mut encoder = opus::Encoder::new(rate, opus::Channels::Mono, opus::Application::Voip)
            .map_err(|e| opus_err("codificador", e))?;
        encoder
            .set_bitrate(opus::Bitrate::Bits(bitrate))
            .map_err(|e| opus_err("taxa de bits", e))?;
        encoder
            .set_signal(opus::Signal::Voice)
            .map_err(|e| opus_err("sinal", e))?;
        let lookahead = encoder
            .get_lookahead()
            .map_err(|e| opus_err("lookahead", e))?
            .max(0) as u64;
        let pre_skip = lookahead * GRANULE_RATE as u64 / rate as u64;

        let file = File::options().write(true).create_new(true).open(path)?;
        // Série do fluxo: só precisa ser diferente entre fluxos encadeados;
        // o relógio basta.
        let serial = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() ^ (d.as_secs() as u32))
            .unwrap_or(0x0015_75E7);
        let mut packets = PacketWriter::new(file);
        packets.write_packet(
            opus_head(rate, pre_skip as u16),
            serial,
            PacketWriteEndInfo::EndPage,
            0,
        )?;
        packets.write_packet(opus_tags(), serial, PacketWriteEndInfo::EndPage, 0)?;
        packets.inner_mut().sync_data()?;
        Ok(Self {
            packets,
            encoder,
            serial,
            rate,
            frame_len: (rate * FRAME_MS / 1000) as usize,
            pending: Vec::new(),
            pre_skip,
            samples_in: 0,
            frames: 0,
            packets_in_page: 0,
            pages_since_sync: 0,
        })
    }

    /// Taxa de entrada, em Hz.
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Segundos de áudio recebidos até agora.
    pub fn elapsed_secs(&self) -> f64 {
        self.samples_in as f64 / self.rate as f64
    }

    /// Acrescenta PCM mono 16 bits. Codifica cada 20 ms completos; o resto
    /// espera o próximo pedaço.
    pub fn write(&mut self, samples: &[i16]) -> Result<()> {
        self.samples_in += samples.len() as u64;
        self.pending.extend_from_slice(samples);
        let mut start = 0;
        while self.pending.len() - start >= self.frame_len {
            let frame = self.pending[start..start + self.frame_len].to_vec();
            start += self.frame_len;
            self.encode_frame(&frame, None)?;
        }
        self.pending.drain(..start);
        Ok(())
    }

    /// Mesmo que [`Self::write`], com o PCM em bytes little-endian (é como o
    /// `AudioRecord` do Android entrega).
    pub fn write_le_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        let samples: Vec<i16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes(*b))
            .collect();
        self.write(&samples)
    }

    fn encode_frame(&mut self, frame: &[i16], final_granule: Option<u64>) -> Result<()> {
        let data = self
            .encoder
            .encode_vec(frame, MAX_PACKET)
            .map_err(|e| opus_err("codificação", e))?;
        self.frames += 1;
        self.packets_in_page += 1;
        let granule = final_granule.unwrap_or(self.frames * FRAME_48K);
        let end = if final_granule.is_some() {
            PacketWriteEndInfo::EndStream
        } else if self.packets_in_page >= PACKETS_PER_PAGE {
            PacketWriteEndInfo::EndPage
        } else {
            PacketWriteEndInfo::NormalPacket
        };
        let closes_page = !matches!(end, PacketWriteEndInfo::NormalPacket);
        self.packets.write_packet(data, self.serial, end, granule)?;
        if closes_page {
            self.packets_in_page = 0;
            self.pages_since_sync += 1;
            if self.pages_since_sync >= SYNC_EVERY_PAGES {
                self.packets.inner_mut().sync_data()?;
                self.pages_since_sync = 0;
            }
        }
        Ok(())
    }

    /// Fecha o fluxo: o último pedaço vai completado com silêncio, e a
    /// posição final descarta esse enchimento. Devolve a duração, em
    /// segundos.
    pub fn finish(mut self) -> Result<f64> {
        let mut last = std::mem::take(&mut self.pending);
        last.resize(self.frame_len, 0);
        let real_48k = self.samples_in * GRANULE_RATE as u64 / self.rate as u64;
        let final_granule = self.pre_skip + real_48k;
        self.encode_frame(&last, Some(final_granule))?;
        let file = self.packets.into_inner();
        file.sync_all()?;
        Ok(real_48k as f64 / GRANULE_RATE as f64)
    }
}

/// Cabeçalho de identificação (RFC 7845 §5.1): mono, família de mapeamento 0.
fn opus_head(rate: u32, pre_skip: u16) -> Vec<u8> {
    let mut h = Vec::with_capacity(19);
    h.extend_from_slice(b"OpusHead");
    h.push(1); // versão
    h.push(1); // canais
    h.extend_from_slice(&pre_skip.to_le_bytes());
    h.extend_from_slice(&rate.to_le_bytes());
    h.extend_from_slice(&0i16.to_le_bytes()); // ganho
    h.push(0); // mapeamento
    h
}

/// Cabeçalho de comentários (RFC 7845 §5.2), sem comentários de usuário.
fn opus_tags() -> Vec<u8> {
    let vendor = format!("ISPer (libopus {})", opus::version());
    let mut t = Vec::with_capacity(16 + vendor.len());
    t.extend_from_slice(b"OpusTags");
    t.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    t.extend_from_slice(vendor.as_bytes());
    t.extend_from_slice(&0u32.to_le_bytes());
    t
}

/// O que o cabeçalho de um Ogg/Opus diz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Head {
    channels: u8,
    pre_skip: u64,
    input_rate: u32,
}

fn parse_head(data: &[u8]) -> Result<Head> {
    if data.len() < 19 || &data[..8] != b"OpusHead" {
        return Err(IsperError::Decode(
            "o arquivo não começa com um cabeçalho Opus (OpusHead)".into(),
        ));
    }
    let channels = data[9];
    if channels == 0 || channels > 2 {
        return Err(IsperError::Decode(format!(
            "Opus com {channels} canais — o ISPer lê mono ou estéreo"
        )));
    }
    Ok(Head {
        channels,
        pre_skip: u16::from_le_bytes([data[10], data[11]]) as u64,
        input_rate: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
    })
}

fn open_reader(path: &Path) -> Result<(PacketReader<BufReader<File>>, u64)> {
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    Ok((PacketReader::new(BufReader::new(file)), len))
}

/// Lê o próximo pacote, tratando o fim do arquivo e uma página cortada no meio
/// (o que sobra de uma gravação interrompida) como fim do fluxo.
fn next_packet(reader: &mut PacketReader<BufReader<File>>) -> Option<ogg::Packet> {
    match reader.read_packet() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Ogg/Opus termina num trecho ilegível ({e}) — usando o que foi lido");
            None
        }
    }
}

/// Duração de um Ogg/Opus pela posição da última página íntegra, sem
/// decodificar o áudio — rápido o bastante para listar gravações.
pub fn duration_secs(path: &Path) -> Result<f64> {
    let (mut reader, _) = open_reader(path)?;
    let head = next_packet(&mut reader)
        .ok_or_else(|| IsperError::Decode("arquivo Opus vazio".into()))
        .and_then(|p| parse_head(&p.data))?;
    let mut last_granule = 0u64;
    while let Some(p) = next_packet(&mut reader) {
        // Os cabeçalhos ficam na posição 0; -1 (u64::MAX) é "sem posição".
        if p.absgp_page() != u64::MAX {
            last_granule = last_granule.max(p.absgp_page());
        }
    }
    Ok(last_granule.saturating_sub(head.pre_skip) as f64 / GRANULE_RATE as f64)
}

/// Decodifica um Ogg/Opus inteiro para 16 kHz mono, em fluxo (o mesmo
/// contrato de [`crate::decode::decode_to_16k`]). Um arquivo truncado vale
/// até a última página íntegra.
pub fn decode_to_16k(
    path: &Path,
    progress: Option<DecodeProgress<'_>>,
    cancel: Option<&AtomicBool>,
) -> Result<DecodedAudio> {
    let (mut reader, len) = open_reader(path)?;
    let head = next_packet(&mut reader)
        .ok_or_else(|| IsperError::Decode("arquivo Opus vazio".into()))
        .and_then(|p| parse_head(&p.data))?;
    // O segundo pacote são os comentários (OpusTags).
    let _ = next_packet(&mut reader);

    let channels = if head.channels == 2 {
        opus::Channels::Stereo
    } else {
        opus::Channels::Mono
    };
    let mut decoder =
        opus::Decoder::new(GRANULE_RATE, channels).map_err(|e| opus_err("decodificador", e))?;
    let mut resampler = Resampler16k::new(GRANULE_RATE)?;
    let mut out: Vec<f32> = Vec::new();
    let mut buf = vec![0f32; MAX_FRAME_48K * head.channels as usize];
    // Amostras a 48 kHz (por canal) que o decodificador já devolveu, com o
    // pre-skip incluído — é a mesma conta da posição (granule) do Ogg.
    let mut decoded = 0u64;
    let mut packets = 0usize;
    let mut bad_packets = 0usize;
    let mut last_reported = -1.0f32;

    while let Some(packet) = next_packet(&mut reader) {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(IsperError::Cancelled);
        }
        packets += 1;
        let n = match decoder.decode_float(&packet.data, &mut buf, false) {
            Ok(n) => n as u64,
            Err(_) => {
                bad_packets += 1;
                continue;
            }
        };
        let start_in_packet = head.pre_skip.saturating_sub(decoded).min(n);
        let mut end_in_packet = n;
        // A última página diz onde o áudio de verdade acaba: o resto do
        // último pacote é enchimento do codificador.
        if packet.last_in_stream() && packet.absgp_page() != u64::MAX {
            end_in_packet = packet.absgp_page().saturating_sub(decoded).min(n);
        }
        decoded += n;
        if end_in_packet > start_in_packet {
            let ch = head.channels as usize;
            let frames = &buf[start_in_packet as usize * ch..end_in_packet as usize * ch];
            let mono = to_mono(frames, head.channels as u16);
            resampler.push(&mono, &mut out)?;
        }
        if let Some(p) = progress
            && len > 0
        {
            let pos = reader.get_mut().stream_position().unwrap_or(0);
            let frac = (pos as f32 / len as f32).min(1.0);
            if frac - last_reported >= 0.01 {
                p(frac);
                last_reported = frac;
            }
        }
    }
    resampler.finish(&mut out)?;
    if out.is_empty() {
        return Err(IsperError::Decode(
            if packets > 0 && bad_packets == packets {
                "nenhum pacote Opus pôde ser decodificado (arquivo corrompido?)".into()
            } else {
                "o arquivo não tem áudio".into()
            },
        ));
    }
    if bad_packets > 0 {
        tracing::warn!(
            bad_packets,
            packets,
            "pacotes Opus com defeito foram pulados"
        );
    }
    if let Some(p) = progress {
        p(1.0);
    }
    Ok(DecodedAudio {
        samples_16k: out,
        source_rate: if head.input_rate > 0 {
            head.input_rate
        } else {
            GRANULE_RATE
        },
        source_channels: head.channels as u16,
        bad_packets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name)
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("isper-opus-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        let _ = std::fs::remove_file(&p);
        p
    }

    /// A fala de referência em 16 kHz, como i16.
    fn fala_i16() -> Vec<i16> {
        crate::audio::load_wav(&fixture("fala-16k.wav"))
            .unwrap()
            .into_whisper_input()
            .unwrap()
            .iter()
            .map(|s| (s * i16::MAX as f32) as i16)
            .collect()
    }

    fn envelope(samples: &[f32]) -> Vec<f32> {
        samples
            .chunks(320)
            .map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt())
            .collect()
    }

    fn correlacao(a: &[f32], b: &[f32]) -> f32 {
        let n = a.len().min(b.len());
        let (a, b) = (&a[..n], &b[..n]);
        let ma = a.iter().sum::<f32>() / n as f32;
        let mb = b.iter().sum::<f32>() / n as f32;
        let (mut cov, mut va, mut vb) = (0f32, 0f32, 0f32);
        for (x, y) in a.iter().zip(b) {
            cov += (x - ma) * (y - mb);
            va += (x - ma).powi(2);
            vb += (y - mb).powi(2);
        }
        cov / (va.sqrt() * vb.sqrt()).max(f32::EPSILON)
    }

    #[test]
    fn grava_e_le_de_volta_o_mesmo_audio() {
        let pcm = fala_i16();
        let path = temp("ida-e-volta.opus");
        let mut w = OggOpusWriter::create(&path, 16_000, DEFAULT_BITRATE).unwrap();
        // Em pedaços de tamanho qualquer, como o AudioRecord entrega.
        for chunk in pcm.chunks(1_234) {
            w.write(chunk).unwrap();
        }
        let secs = w.finish().unwrap();
        let esperado = pcm.len() as f64 / 16_000.0;
        assert!((secs - esperado).abs() < 1e-6, "{secs} vs {esperado}");
        assert!((duration_secs(&path).unwrap() - esperado).abs() < 0.001);

        let d = decode_to_16k(&path, None, None).unwrap();
        assert_eq!(d.source_rate, 16_000);
        assert_eq!(d.source_channels, 1);
        assert!(
            (d.duration_secs() as f64 - esperado).abs() < 0.01,
            "{} vs {esperado}",
            d.duration_secs()
        );
        let original: Vec<f32> = pcm.iter().map(|s| *s as f32 / i16::MAX as f32).collect();
        let c = correlacao(&envelope(&original), &envelope(&d.samples_16k));
        assert!(c > 0.97, "correlação {c}");
        // 32 kbit/s: ~4 KB por segundo, mais os cabeçalhos das páginas.
        let bytes = std::fs::metadata(&path).unwrap().len() as f64;
        assert!(bytes / esperado < 5_000.0, "{bytes} bytes");
    }

    #[test]
    fn arquivo_cortado_no_meio_vale_ate_a_ultima_pagina() {
        let pcm = fala_i16();
        let path = temp("inteiro.opus");
        let mut w = OggOpusWriter::create(&path, 16_000, DEFAULT_BITRATE).unwrap();
        w.write(&pcm).unwrap();
        let total = w.finish().unwrap();

        // O app morre no meio de uma página: sobra 60% do arquivo.
        let bytes = std::fs::read(&path).unwrap();
        let cortado = temp("cortado.opus");
        std::fs::write(&cortado, &bytes[..bytes.len() * 6 / 10]).unwrap();

        let secs = duration_secs(&cortado).unwrap();
        assert!(
            secs > total * 0.4 && secs < total * 0.7,
            "{secs} de {total}"
        );
        let d = decode_to_16k(&cortado, None, None).unwrap();
        // O decodificado vai até a última página inteira; a posição dela é o
        // fim do último pacote completo (menos o pre-skip).
        assert!(
            (d.duration_secs() as f64 - secs).abs() < 1.1,
            "{} vs {secs}",
            d.duration_secs()
        );
    }

    #[test]
    fn nunca_sobrescreve_e_recusa_taxa_invalida() {
        let path = temp("existe.opus");
        std::fs::write(&path, b"x").unwrap();
        assert!(OggOpusWriter::create(&path, 16_000, DEFAULT_BITRATE).is_err());
        let novo = temp("taxa.opus");
        let e = OggOpusWriter::create(&novo, 44_100, DEFAULT_BITRATE)
            .err()
            .unwrap();
        assert!(e.to_string().contains("44100"), "{e}");
    }

    #[test]
    fn le_o_opus_de_fora() {
        // Uma mensagem de voz de 2 s gerada por outro codificador (ffmpeg).
        let d = decode_to_16k(&fixture("formatos/fala-2s.opus"), None, None).unwrap();
        assert!(
            (d.duration_secs() - 2.0).abs() < 0.1,
            "{} s",
            d.duration_secs()
        );
        assert_eq!(d.bad_packets, 0);
    }

    #[test]
    fn gravacao_de_48k_em_pedacos_pequenos() {
        // O celular grava a 48 kHz e entrega 100 ms por vez.
        let path = temp("48k.opus");
        let mut w = OggOpusWriter::create(&path, 48_000, DEFAULT_BITRATE).unwrap();
        let tom: Vec<i16> = (0..48_000 * 3)
            .map(|i| ((i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 8_000.0) as i16)
            .collect();
        for chunk in tom.chunks(4_800) {
            let bytes: Vec<u8> = chunk.iter().flat_map(|s| s.to_le_bytes()).collect();
            w.write_le_bytes(&bytes).unwrap();
        }
        assert!((w.elapsed_secs() - 3.0).abs() < 1e-9);
        assert!((w.finish().unwrap() - 3.0).abs() < 1e-9);
        assert!((duration_secs(&path).unwrap() - 3.0).abs() < 0.001);
        let d = decode_to_16k(&path, None, None).unwrap();
        assert!(
            (d.duration_secs() - 3.0).abs() < 0.01,
            "{}",
            d.duration_secs()
        );
        assert_eq!(d.source_rate, 48_000);
    }
}
