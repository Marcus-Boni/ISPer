//! `isper-cli receber` e `isper-cli enviar`: os dois lados da sincronia com o
//! celular (Fase 9.3), sem o app desktop nem o Android.
//!
//! - `receber` é um PC sem interface: mostra o QR no terminal, aceita o
//!   pareamento e guarda as gravações numa pasta. É o que o e2e do Android
//!   usa (`tools/e2e/android-sync.ps1`), e serve a quem só quer o arquivo.
//!   Uma gravação vira "pronta" quando aparece um `<id>.ata.md` ao lado dela.
//! - `enviar` faz o papel do celular: pareia com um código e manda um áudio.
//!   É o que o e2e do app desktop usa (`tools/e2e/sync.ps1`).

use std::collections::HashMap;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::{Context, bail};
use isper_sync::client::{Client, PairedPc};
use isper_sync::code::PairingCode;
use isper_sync::proto::{Minutes, RecordingOffer, RemoteState};
use isper_sync::server::{Approval, Host, Server, ServerConfig};
use isper_sync::{EndpointId, RelayUrl, SecretKey};

fn runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .context("runtime do tokio")
}

/// A chave de um lado, guardada em hexadecimal num arquivo.
fn load_or_create_key(path: &Path) -> anyhow::Result<SecretKey> {
    if let Ok(text) = std::fs::read_to_string(path) {
        return SecretKey::from_str(text.trim()).context("chave corrompida");
    }
    let key = SecretKey::generate();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(
        path,
        key.to_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
    )?;
    Ok(key)
}

// ------------------------------------------------------------------ receber

/// O PC sem interface: os aparelhos em `aparelhos.json`, as gravações em
/// `<pasta>/<aparelho>/`.
struct FolderHost {
    dir: PathBuf,
    approve_all: bool,
    devices: Mutex<HashMap<String, String>>,
}

impl FolderHost {
    fn devices_file(&self) -> PathBuf {
        self.dir.join("aparelhos.json")
    }

    fn save(&self, devices: &HashMap<String, String>) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(devices).map_err(std::io::Error::other)?;
        std::fs::write(self.devices_file(), json)
    }

    fn minutes_path(&self, device: &EndpointId, id: &str) -> PathBuf {
        self.inbox(device).join(format!("{id}.ata.md"))
    }

    fn has_recording(&self, device: &EndpointId, id: &str) -> bool {
        let Ok(entries) = std::fs::read_dir(self.inbox(device)) else {
            return false;
        };
        entries.flatten().any(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.starts_with(&format!("{id}."))
                && !name.ends_with(".part")
                && !name.ends_with(".offer.json")
                && !name.ends_with(".ata.md")
        })
    }
}

impl Host for FolderHost {
    fn pc_name(&self) -> String {
        std::env::var("COMPUTERNAME").unwrap_or_else(|_| "isper-cli".into())
    }

    fn is_paired(&self, device: &EndpointId) -> bool {
        self.devices
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(&device.to_string())
    }

    fn approve(&self, device: EndpointId, name: String) -> Approval {
        if self.approve_all {
            println!(
                "pareando com {name} ({}): aprovado (--aprovar)",
                device.fmt_short()
            );
            return Box::pin(async { true });
        }
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                print!(
                    "Novo aparelho: {name} ({}). Permitir? [s/N] ",
                    device.fmt_short()
                );
                let _ = std::io::stdout().flush();
                let mut line = String::new();
                std::io::stdin().read_line(&mut line).is_ok()
                    && matches!(line.trim().to_lowercase().as_str(), "s" | "sim" | "y")
            })
            .await
            .unwrap_or(false)
        })
    }

    fn add_device(&self, device: EndpointId, name: &str) -> std::io::Result<()> {
        let mut devices = self.devices.lock().unwrap_or_else(PoisonError::into_inner);
        devices.insert(device.to_string(), name.to_string());
        self.save(&devices)
    }

    fn forget_device(&self, device: &EndpointId) {
        let mut devices = self.devices.lock().unwrap_or_else(PoisonError::into_inner);
        devices.remove(&device.to_string());
        let _ = self.save(&devices);
        println!("aparelho {} esquecido", device.fmt_short());
    }

    fn inbox(&self, device: &EndpointId) -> PathBuf {
        self.dir.join(device.fmt_short().to_string())
    }

    fn received(&self, device: &EndpointId, offer: &RecordingOffer, path: &Path) -> RemoteState {
        println!(
            "recebida: {} ({} bytes, {} momento(s)) de {}",
            path.display(),
            offer.size,
            offer.moments.len(),
            device.fmt_short()
        );
        // O manifesto ao lado, para o teste conferir o que o celular mandou.
        if let Ok(json) = serde_json::to_vec_pretty(offer) {
            let _ = std::fs::write(path.with_extension("offer.json"), json);
        }
        self.state(device, &offer.id)
    }

    fn state(&self, device: &EndpointId, id: &str) -> RemoteState {
        if let Some(m) = self.minutes(device, id) {
            return RemoteState::Done { title: m.title };
        }
        if self.has_recording(device, id) {
            RemoteState::Queued
        } else {
            RemoteState::Unknown
        }
    }

    fn minutes(&self, device: &EndpointId, id: &str) -> Option<Minutes> {
        let markdown = std::fs::read_to_string(self.minutes_path(device, id)).ok()?;
        // Uma ata escrita no Windows pode vir com BOM, e aí o "# " da primeira
        // linha não é reconhecido.
        let markdown = markdown.trim_start_matches('\u{feff}').to_string();
        let title = markdown
            .lines()
            .find_map(|l| l.strip_prefix("# "))
            .unwrap_or(id)
            .trim()
            .to_string();
        Some(Minutes {
            title,
            started_at: String::new(),
            markdown,
        })
    }
}

pub(crate) fn receive(
    dir: &Path,
    port: u16,
    announce: &[SocketAddr],
    approve_all: bool,
    relay: Option<&str>,
    valid_secs: u64,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let key = load_or_create_key(&dir.join("chave-do-pc.txt"))?;
    let devices: HashMap<String, String> = std::fs::read(dir.join("aparelhos.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let host = Arc::new(FolderHost {
        dir: dir.to_path_buf(),
        approve_all,
        devices: Mutex::new(devices),
    });
    let relay = relay
        .map(RelayUrl::from_str)
        .transpose()
        .context("relay inválido")?;
    let rt = runtime()?;
    rt.block_on(async {
        let server = Server::start(
            ServerConfig {
                secret_key: key,
                port,
                relay,
                mdns: true,
            },
            host,
        )
        .await?;
        let mut code = server.start_pairing_for(Duration::from_secs(valid_secs))?;
        if !announce.is_empty() {
            code.addrs = announce.to_vec();
        }
        let uri = code.to_uri();
        std::fs::write(dir.join("codigo.txt"), &uri)?;
        if let Ok(qr) = qrcode::QrCode::new(uri.as_bytes()) {
            let art = qr
                .render::<qrcode::render::unicode::Dense1x2>()
                .quiet_zone(true)
                .build();
            println!("{art}");
        }
        println!(
            "PC {} ouvindo na porta {:?}",
            server.id().fmt_short(),
            server.port()
        );
        println!("código de pareamento (vale {valid_secs} s; também em codigo.txt):\n{uri}");
        println!("Ctrl+C para parar.");
        tokio::signal::ctrl_c().await?;
        server.shutdown().await;
        anyhow::Ok(())
    })
}

// ------------------------------------------------------------------- enviar

pub(crate) struct SendArgs<'a> {
    pub(crate) file: &'a Path,
    pub(crate) code: Option<&'a str>,
    pub(crate) state_dir: &'a Path,
    pub(crate) wait_secs: u64,
    pub(crate) name: &'a str,
    pub(crate) id: Option<&'a str>,
    pub(crate) moments: Vec<f64>,
}

pub(crate) fn send(args: SendArgs<'_>) -> anyhow::Result<()> {
    std::fs::create_dir_all(args.state_dir)?;
    let key = load_or_create_key(&args.state_dir.join("chave-do-celular.txt"))?;
    let pc_file = args.state_dir.join("pc.json");
    let rt = runtime()?;
    rt.block_on(async {
        let code = args.code.map(PairingCode::parse).transpose()?;
        let saved: Option<PairedPc> = std::fs::read(&pc_file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        let relay = code
            .as_ref()
            .and_then(|c| c.relay.clone())
            .or_else(|| saved.as_ref().and_then(PairedPc::relay_url));
        let client = Client::bind(key, relay, true).await?;
        let pc = match (code, saved) {
            (Some(code), _) => {
                println!("pareando com {}… (confirme no PC)", code.pc_name);
                let pc = client
                    .pair(&code, args.name, env!("CARGO_PKG_VERSION"))
                    .await?;
                std::fs::write(&pc_file, serde_json::to_vec_pretty(&pc)?)?;
                println!("pareado com {}", pc.name);
                pc
            }
            (None, Some(pc)) => pc,
            (None, None) => bail!("nenhum PC pareado: passe --codigo com o texto do QR"),
        };

        let mut offer = offer_for(args.file, args.id)?;
        offer.moments = args.moments.clone();
        let session = client.connect(&pc).await?;
        session.hello(args.name, env!("CARGO_PKG_VERSION")).await?;
        let (have, mut state) = session.offer(&offer).await?;
        if have < offer.size {
            println!(
                "enviando {} ({} de {} bytes já no PC)",
                offer.id, have, offer.size
            );
            let started = std::time::Instant::now();
            let progress = |sent: u64, total: u64| {
                if sent == total || sent % (4 * 1024 * 1024) < 256 * 1024 {
                    print!("\r  {:>3}%", sent * 100 / total.max(1));
                    let _ = std::io::stdout().flush();
                }
            };
            state = session
                .upload(args.file, &offer, have, &progress, &AtomicBool::new(false))
                .await?;
            println!("\r  enviado em {:.1} s", started.elapsed().as_secs_f64());
        }
        println!("estado no PC: {state:?}");

        let deadline = std::time::Instant::now() + Duration::from_secs(args.wait_secs);
        while !state.is_final() && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_secs(2)).await;
            state = session
                .status(vec![offer.id.clone()])
                .await?
                .into_iter()
                .next()
                .map(|i| i.state)
                .unwrap_or(RemoteState::Unknown);
        }
        match &state {
            RemoteState::Done { title } => {
                let minutes = session
                    .minutes(&offer.id)
                    .await?
                    .context("o PC disse que a ata estava pronta, mas não a mandou")?;
                let out = args.file.with_extension("ata.md");
                std::fs::write(&out, &minutes.markdown)?;
                println!("ata pronta: {title} → {}", out.display());
            }
            RemoteState::Failed { reason } => bail!("o PC não conseguiu processar: {reason}"),
            _ if args.wait_secs > 0 => bail!("a ata não ficou pronta em {} s", args.wait_secs),
            _ => {}
        }
        session.close();
        client.close().await;
        anyhow::Ok(())
    })
}

/// O anúncio de um arquivo: id pelo nome, data pela modificação.
fn offer_for(file: &Path, id: Option<&str>) -> anyhow::Result<RecordingOffer> {
    let meta = std::fs::metadata(file).with_context(|| format!("{}", file.display()))?;
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let id = id.map(str::to_string).unwrap_or_else(|| {
        stem.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .take(64)
            .collect()
    });
    if !isper_sync::check_id(&id) {
        bail!("id inválido para a gravação: {id:?} (use --id)");
    }
    let extension = file
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| "opus".into());
    let started_at = meta
        .modified()
        .map(chrono::DateTime::<chrono::Local>::from)
        .unwrap_or_else(|_| chrono::Local::now())
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
    let duration_secs = if extension == "opus" {
        isper_core::ogg_opus::duration_secs(file).ok()
    } else {
        None
    };
    Ok(RecordingOffer {
        id,
        size: meta.len(),
        sha256: isper_sync::sha256_file(file)?,
        extension,
        started_at,
        duration_secs,
        moments: Vec::new(),
        source_name: file.file_name().map(|n| n.to_string_lossy().into_owned()),
    })
}
