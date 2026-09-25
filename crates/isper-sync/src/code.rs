//! O código de pareamento: o texto dentro do QR que o PC mostra.
//!
//! ```text
//! isper://parear?v=1&pc=<chave do PC>&s=<segredo>&n=<nome>&a=<ip:porta>,<ip:porta>&r=<relay>
//! ```
//!
//! - `pc`: a chave pública do PC (64 hexadecimais) — o celular só aceita
//!   conversar com ela;
//! - `s`: um segredo aleatório de 128 bits, de uso único, que vale por
//!   [`crate::PAIRING_TTL`];
//! - `n`: o nome do PC, para o celular mostrar;
//! - `a`: os endereços diretos na rede local, na ordem de preferência;
//! - `r`: o relay, se o PC usa um (opcional).
//!
//! O mesmo texto pode ser colado no celular em vez de lido pela câmera.

use std::net::SocketAddr;
use std::str::FromStr;

use iroh::{EndpointAddr, EndpointId, RelayUrl};

use crate::util::{hex, pct_decode, pct_encode, unhex};
use crate::{Result, SyncError};

const PREFIX: &str = "isper://parear?";
const VERSION: &str = "1";
/// Quantos endereços diretos cabem no QR sem deixá-lo difícil de ler.
pub const MAX_ADDRS: usize = 4;

/// O conteúdo do QR de pareamento.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairingCode {
    pub pc: EndpointId,
    pub pc_name: String,
    pub addrs: Vec<SocketAddr>,
    pub relay: Option<RelayUrl>,
    pub secret: [u8; 16],
}

impl PairingCode {
    /// O texto do QR.
    pub fn to_uri(&self) -> String {
        let addrs: Vec<String> = self
            .addrs
            .iter()
            .take(MAX_ADDRS)
            .map(SocketAddr::to_string)
            .collect();
        let mut uri = format!(
            "{PREFIX}v={VERSION}&pc={}&s={}&n={}&a={}",
            self.pc,
            hex(&self.secret),
            pct_encode(&self.pc_name),
            addrs.join(",")
        );
        if let Some(relay) = &self.relay {
            uri.push_str("&r=");
            uri.push_str(&pct_encode(relay.as_str()));
        }
        uri
    }

    /// Lê o texto do QR (ou colado). Aceita espaços e quebras de linha em
    /// volta, que aparecem quando o código passa por uma mensagem.
    pub fn parse(text: &str) -> Result<Self> {
        let bad = |why: &str| SyncError::BadCode(why.to_string());
        let text = text.trim();
        let query = text
            .strip_prefix(PREFIX)
            .ok_or_else(|| bad("não é um código do ISPer"))?;
        let (mut v, mut pc, mut s, mut n, mut a, mut r) = (None, None, None, None, None, None);
        for pair in query.split('&') {
            let (k, val) = pair.split_once('=').unwrap_or((pair, ""));
            let slot = match k {
                "v" => &mut v,
                "pc" => &mut pc,
                "s" => &mut s,
                "n" => &mut n,
                "a" => &mut a,
                "r" => &mut r,
                _ => continue, // campos novos de versões futuras
            };
            *slot = Some(val);
        }
        if v != Some(VERSION) {
            return Err(bad("versão do código desconhecida (atualize o app)"));
        }
        let pc = EndpointId::from_str(pc.ok_or_else(|| bad("falta a chave do PC"))?)
            .map_err(|_| bad("chave do PC inválida"))?;
        let secret: [u8; 16] = s
            .and_then(unhex)
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| bad("segredo inválido"))?;
        let pc_name = n
            .and_then(pct_decode)
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| "PC".to_string());
        let addrs = a
            .unwrap_or("")
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| SocketAddr::from_str(s).map_err(|_| bad("endereço inválido")))
            .collect::<Result<Vec<_>>>()?;
        let relay = match r.and_then(pct_decode) {
            Some(url) if !url.is_empty() => {
                Some(RelayUrl::from_str(&url).map_err(|_| bad("relay inválido"))?)
            }
            _ => None,
        };
        if addrs.is_empty() && relay.is_none() {
            return Err(bad("o código não diz como chegar ao PC"));
        }
        Ok(Self {
            pc,
            pc_name,
            addrs,
            relay,
            secret,
        })
    }

    /// Como discar o PC: a chave, os endereços diretos e o relay.
    pub fn endpoint_addr(&self) -> EndpointAddr {
        dial_addr(self.pc, &self.addrs, self.relay.as_ref())
    }
}

pub(crate) fn dial_addr(
    id: EndpointId,
    addrs: &[SocketAddr],
    relay: Option<&RelayUrl>,
) -> EndpointAddr {
    let mut addr = EndpointAddr::new(id);
    for a in addrs {
        addr = addr.with_ip_addr(*a);
    }
    if let Some(r) = relay {
        addr = addr.with_relay_url(r.clone());
    }
    addr
}

/// Os endereços que valem a pena pôr no QR: IPv4 de rede privada primeiro,
/// depois os demais IPv4 e os IPv6 globais; sem loopback nem link-local.
pub fn preferred_addrs(all: impl IntoIterator<Item = SocketAddr>) -> Vec<SocketAddr> {
    let mut v: Vec<SocketAddr> = all
        .into_iter()
        .filter(|a| {
            let ip = a.ip();
            !ip.is_loopback()
                && !ip.is_unspecified()
                && match ip {
                    std::net::IpAddr::V4(v4) => !v4.is_link_local(),
                    std::net::IpAddr::V6(v6) => !v6.is_unicast_link_local(),
                }
        })
        .collect();
    v.sort_by_key(|a| match a.ip() {
        std::net::IpAddr::V4(v4) if v4.is_private() => 0,
        std::net::IpAddr::V4(_) => 1,
        std::net::IpAddr::V6(_) => 2,
    });
    v.dedup();
    v.truncate(MAX_ADDRS);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    fn sample() -> PairingCode {
        PairingCode {
            pc: SecretKey::generate().public(),
            pc_name: "MARCUS-PC · sala 2".into(),
            addrs: vec![
                "192.168.15.5:47823".parse().unwrap(),
                "[2804:1b3:9800::757]:47823".parse().unwrap(),
            ],
            relay: None,
            secret: [7u8; 16],
        }
    }

    #[test]
    fn o_codigo_vai_e_volta() {
        let code = sample();
        let uri = code.to_uri();
        assert!(uri.starts_with("isper://parear?v=1&pc="));
        assert_eq!(PairingCode::parse(&uri).unwrap(), code);
        // Colado de uma mensagem, com espaço e quebra de linha em volta.
        assert_eq!(PairingCode::parse(&format!("  {uri}\n")).unwrap(), code);

        let mut with_relay = sample();
        with_relay.relay = Some("https://relay.exemplo.com.br./".parse().unwrap());
        let back = PairingCode::parse(&with_relay.to_uri()).unwrap();
        assert_eq!(back.relay, with_relay.relay);
    }

    #[test]
    fn codigos_ruins_explicam_o_que_falta() {
        let uri = sample().to_uri();
        for (ruim, trecho) in [
            ("https://isper.pages.dev", "não é um código"),
            (&uri.replace("v=1", "v=9"), "versão"),
            (&uri.replace("&s=", "&s=zz"), "segredo"),
            (&uri.replace("&pc=", "&pc=00"), "chave do PC"),
            (
                &uri.replace("192.168.15.5:47823", "192.168.15.5"),
                "endereço",
            ),
        ] {
            let e = PairingCode::parse(ruim).unwrap_err().to_string();
            assert!(e.contains(trecho), "{ruim}: {e}");
        }
        let mut sem_rota = sample();
        sem_rota.addrs.clear();
        let e = PairingCode::parse(&sem_rota.to_uri())
            .unwrap_err()
            .to_string();
        assert!(e.contains("como chegar"), "{e}");
    }

    #[test]
    fn enderecos_do_qr_na_ordem_certa() {
        let all: Vec<SocketAddr> = [
            "[2804:1b3::1]:47823",
            "127.0.0.1:47823",
            "[fe80::1]:47823",
            "8.8.8.8:47823",
            "192.168.15.5:47823",
            "192.168.15.5:47823",
            "0.0.0.0:47823",
        ]
        .iter()
        .map(|s| s.parse().unwrap())
        .collect();
        let v = preferred_addrs(all);
        let s: Vec<String> = v.iter().map(ToString::to_string).collect();
        assert_eq!(
            s,
            ["192.168.15.5:47823", "8.8.8.8:47823", "[2804:1b3::1]:47823"]
        );
    }
}
