//! O protocolo de ponta a ponta: um servidor e um cliente de verdade,
//! conversando pelo iroh na própria máquina (sem relay, sem rede externa).

// O host e os ajudantes de teste também são teste: o pânico é o relatório
// (o `allow-unwrap-in-tests` do clippy.toml só cobre as funções `#[test]`).
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use isper_sync::client::Client;
use isper_sync::code::PairingCode;
use isper_sync::proto::{Minutes, RecordingOffer, RemoteState};
use isper_sync::server::{Approval, Host, Server, ServerConfig};
use isper_sync::{EndpointId, SecretKey, SyncError, sha256_file};

/// Um PC de mentira: aparelhos e gravações em memória.
struct TestHost {
    inbox: PathBuf,
    approve: AtomicBool,
    devices: Mutex<HashMap<EndpointId, String>>,
    received: Mutex<HashMap<String, (RecordingOffer, PathBuf)>>,
    done: Mutex<HashMap<String, String>>,
}

impl TestHost {
    fn new(inbox: &Path) -> Arc<Self> {
        Arc::new(Self {
            inbox: inbox.to_path_buf(),
            approve: AtomicBool::new(true),
            devices: Mutex::default(),
            received: Mutex::default(),
            done: Mutex::default(),
        })
    }

    /// "O PC processou": a gravação vira ata.
    fn finish(&self, id: &str, title: &str) {
        self.done
            .lock()
            .unwrap()
            .insert(id.to_string(), title.to_string());
    }
}

impl Host for TestHost {
    fn pc_name(&self) -> String {
        "PC-DE-TESTE".into()
    }
    fn is_paired(&self, device: &EndpointId) -> bool {
        self.devices.lock().unwrap().contains_key(device)
    }
    fn approve(&self, _device: EndpointId, _name: String) -> Approval {
        let ok = self.approve.load(Ordering::Relaxed);
        Box::pin(async move { ok })
    }
    fn add_device(&self, device: EndpointId, name: &str) -> std::io::Result<()> {
        self.devices
            .lock()
            .unwrap()
            .insert(device, name.to_string());
        Ok(())
    }
    fn forget_device(&self, device: &EndpointId) {
        self.devices.lock().unwrap().remove(device);
    }
    fn inbox(&self, device: &EndpointId) -> PathBuf {
        self.inbox.join(device.fmt_short().to_string())
    }
    fn received(&self, _device: &EndpointId, offer: &RecordingOffer, path: &Path) -> RemoteState {
        self.received
            .lock()
            .unwrap()
            .insert(offer.id.clone(), (offer.clone(), path.to_path_buf()));
        RemoteState::Queued
    }
    fn state(&self, _device: &EndpointId, id: &str) -> RemoteState {
        if let Some(title) = self.done.lock().unwrap().get(id) {
            return RemoteState::Done {
                title: title.clone(),
            };
        }
        if self.received.lock().unwrap().contains_key(id) {
            return RemoteState::Queued;
        }
        RemoteState::Unknown
    }
    fn minutes(&self, _device: &EndpointId, id: &str) -> Option<Minutes> {
        let title = self.done.lock().unwrap().get(id)?.clone();
        Some(Minutes {
            title: title.clone(),
            started_at: "24/09/2026 20:12".into(),
            markdown: format!("# {title}\n\n**Participante 1:** bom dia.\n"),
        })
    }
}

async fn server(dir: &Path) -> (Server<TestHost>, Arc<TestHost>) {
    let host = TestHost::new(dir);
    let server = Server::start(
        ServerConfig {
            secret_key: SecretKey::generate(),
            port: 0,
            relay: None,
            mdns: false,
        },
        host.clone(),
    )
    .await
    .unwrap();
    (server, host)
}

/// O código como o celular o leria, apontando para o loopback (a máquina
/// do teste pode não ter rede).
fn local_code(server: &Server<TestHost>, code: PairingCode) -> PairingCode {
    let port = server.port().unwrap();
    PairingCode {
        addrs: vec![([127, 0, 0, 1], port).into()],
        ..code
    }
}

fn recording(dir: &Path, id: &str, size: usize) -> (PathBuf, RecordingOffer) {
    let path = dir.join(format!("{id}.opus"));
    let bytes: Vec<u8> = (0..size).map(|i| (i * 7 % 251) as u8).collect();
    std::fs::write(&path, &bytes).unwrap();
    let offer = RecordingOffer {
        id: id.into(),
        size: size as u64,
        sha256: sha256_file(&path).unwrap(),
        extension: "opus".into(),
        started_at: "2026-09-24T20:12:24-03:00".into(),
        duration_secs: Some(48.0),
        moments: vec![9.5],
        source_name: None,
    };
    (path, offer)
}

fn no_progress(_: u64, _: u64) {}

#[tokio::test(flavor = "multi_thread")]
async fn pareia_envia_retoma_e_devolve_a_ata() {
    let tmp = tempfile::tempdir().unwrap();
    let (server, host) = server(&tmp.path().join("pc")).await;
    let code = local_code(&server, server.start_pairing().unwrap());
    // O QR é texto: o que o celular recebe é o que o PC escreveu.
    let code = PairingCode::parse(&code.to_uri()).unwrap();

    let phone = Client::bind(SecretKey::generate(), None, false)
        .await
        .unwrap();
    let pc = phone.pair(&code, "Galaxy Tab A9", "teste").await.unwrap();
    assert_eq!(pc.name, "PC-DE-TESTE");
    assert_eq!(pc.id, server.id().to_string());
    assert_eq!(
        host.devices
            .lock()
            .unwrap()
            .get(&phone.id())
            .map(String::as_str),
        Some("Galaxy Tab A9")
    );
    // O segredo é de uso único.
    assert!(server.pairing_remaining().is_none());

    let (path, offer) = recording(tmp.path(), "20260924-201224", 3 * 1024 * 1024 + 123);
    let session = phone.connect(&pc).await.unwrap();
    let welcome = session.hello("Galaxy Tab A9", "teste").await.unwrap();
    assert_eq!(welcome.pc_name, "PC-DE-TESTE");

    let (have, state) = session.offer(&offer).await.unwrap();
    assert_eq!((have, &state), (0, &RemoteState::Receiving { have: 0 }));

    // O envio é interrompido perto de 1 MB (o celular saiu da rede)...
    let cancel = AtomicBool::new(false);
    let stop_at_1mb = |sent: u64, _| {
        if sent >= 1024 * 1024 {
            cancel.store(true, Ordering::Relaxed);
        }
    };
    let e = session
        .upload(&path, &offer, 0, &stop_at_1mb, &cancel)
        .await
        .unwrap_err();
    assert!(matches!(e, SyncError::Cancelled), "{e}");

    // ... e continua de onde parou.
    let (have, _) = session.offer(&offer).await.unwrap();
    assert!(
        have >= 1024 * 1024 && have < offer.size,
        "o PC tinha {have}"
    );
    let cancel = AtomicBool::new(false);
    let state = session
        .upload(&path, &offer, have, &no_progress, &cancel)
        .await
        .unwrap();
    assert_eq!(state, RemoteState::Queued);

    let (got, stored) = host.received.lock().unwrap()["20260924-201224"].clone();
    assert_eq!(got, offer);
    assert_eq!(
        sha256_file(&stored).unwrap(),
        offer.sha256,
        "chegou íntegro"
    );
    assert_eq!(stored.extension().unwrap(), "opus");

    // Mandar de novo não reenvia: o PC já tem.
    let (have, state) = session.offer(&offer).await.unwrap();
    assert_eq!((have, state), (offer.size, RemoteState::Queued));

    // O PC processa; o celular pergunta e busca a ata.
    let items = session.status(vec![offer.id.clone()]).await.unwrap();
    assert_eq!(items[0].state, RemoteState::Queued);
    assert!(session.minutes(&offer.id).await.unwrap().is_none());
    host.finish(&offer.id, "Visita à fábrica");
    let items = session
        .status(vec![offer.id.clone(), "outra".into()])
        .await
        .unwrap();
    assert_eq!(
        items[0].state,
        RemoteState::Done {
            title: "Visita à fábrica".into()
        }
    );
    assert_eq!(items[1].state, RemoteState::Unknown);
    let minutes = session.minutes(&offer.id).await.unwrap().unwrap();
    assert!(minutes.markdown.starts_with("# Visita à fábrica"));

    session.close();
    phone.close().await;
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn segredo_errado_codigo_vencido_e_recusa_nao_pareiam() {
    let tmp = tempfile::tempdir().unwrap();
    let (server, host) = server(tmp.path()).await;
    let phone = Client::bind(SecretKey::generate(), None, false)
        .await
        .unwrap();

    // Sem pareamento aberto.
    let mut code = local_code(&server, server.start_pairing().unwrap());
    server.cancel_pairing();
    let e = phone.pair(&code, "x", "t").await.unwrap_err();
    assert!(e.to_string().contains("nenhum pareamento aberto"), "{e}");

    // Segredo errado: cinco tentativas e o pareamento fecha.
    let real = local_code(&server, server.start_pairing().unwrap());
    code.secret = [0u8; 16];
    for _ in 0..isper_sync::PAIRING_MAX_ATTEMPTS {
        let e = phone.pair(&code, "x", "t").await.unwrap_err();
        assert!(matches!(e, SyncError::Denied(_)), "{e}");
    }
    let e = phone.pair(&real, "x", "t").await.unwrap_err();
    assert!(e.to_string().contains("nenhum pareamento aberto"), "{e}");

    // Código vencido.
    let short = local_code(
        &server,
        server.start_pairing_for(Duration::from_millis(50)).unwrap(),
    );
    tokio::time::sleep(Duration::from_millis(120)).await;
    let e = phone.pair(&short, "x", "t").await.unwrap_err();
    assert!(e.to_string().contains("venceu"), "{e}");

    // Quem está no PC recusa.
    host.approve.store(false, Ordering::Relaxed);
    let code = local_code(&server, server.start_pairing().unwrap());
    let e = phone.pair(&code, "x", "t").await.unwrap_err();
    assert!(e.to_string().contains("recusado"), "{e}");
    assert!(host.devices.lock().unwrap().is_empty());

    phone.close().await;
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn aparelho_estranho_esquecido_ou_com_arquivo_corrompido_nao_entra() {
    let tmp = tempfile::tempdir().unwrap();
    let (server, host) = server(&tmp.path().join("pc")).await;
    let code = local_code(&server, server.start_pairing().unwrap());
    let phone = Client::bind(SecretKey::generate(), None, false)
        .await
        .unwrap();
    let pc = phone.pair(&code, "Celular", "t").await.unwrap();
    let (path, offer) = recording(tmp.path(), "rec-1", 200_000);

    // Outro aparelho, com o endereço e a chave do PC mas sem pareamento.
    let stranger = Client::bind(SecretKey::generate(), None, false)
        .await
        .unwrap();
    let s = stranger.connect(&pc).await.unwrap();
    assert!(matches!(s.offer(&offer).await, Err(SyncError::NotPaired)));
    assert!(matches!(
        s.status(vec!["rec-1".into()]).await,
        Err(SyncError::NotPaired)
    ));
    stranger.close().await;

    // O SHA-256 anunciado não bate com o que chega: o PC descarta.
    let session = phone.connect(&pc).await.unwrap();
    let wrong = RecordingOffer {
        sha256: "00".repeat(32),
        ..offer.clone()
    };
    session.offer(&wrong).await.unwrap();
    let e = session
        .upload(&path, &wrong, 0, &no_progress, &AtomicBool::new(false))
        .await
        .unwrap_err();
    assert!(e.to_string().contains("chegou diferente"), "{e}");
    let (have, _) = session.offer(&offer).await.unwrap();
    assert_eq!(have, 0, "a parcial corrompida foi descartada");

    // Pedido fora de ordem e id perigoso são recusados.
    let e = session
        .upload(&path, &offer, 999, &no_progress, &AtomicBool::new(false))
        .await
        .unwrap_err();
    assert!(e.to_string().contains("o PC tem 0 bytes"), "{e}");
    let evil = RecordingOffer {
        id: "../../fora".into(),
        ..offer.clone()
    };
    assert!(session.offer(&evil).await.is_err());

    // O celular se despede; depois disso, o PC não o reconhece.
    session.forget().await.unwrap();
    assert!(host.devices.lock().unwrap().is_empty());
    assert!(matches!(
        session.offer(&offer).await,
        Err(SyncError::NotPaired)
    ));

    phone.close().await;
    server.shutdown().await;
}
