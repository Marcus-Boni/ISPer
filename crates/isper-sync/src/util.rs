//! Utilitários pequenos: ids seguros, hexadecimal, SHA-256 e aleatoriedade.

use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

/// Um id de gravação vira nome de arquivo no PC: só letras, dígitos, `-` e
/// `_`, até 64 caracteres (o gravador usa `20260924-201224`).
pub fn check_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    s
}

pub(crate) fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// SHA-256 de um arquivo, em hexadecimal minúsculo — o mesmo que a
/// importação do PC usa para não criar a mesma reunião duas vezes.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

/// Compara sem vazar pelo tempo quantos bytes batem.
pub(crate) fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub(crate) fn random_16() -> std::io::Result<[u8; 16]> {
    let mut buf = [0u8; 16];
    getrandom::fill(&mut buf).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(buf)
}

/// Codifica um valor de parâmetro de URL (tudo fora do conjunto seguro vira `%XX`).
pub(crate) fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push_str(&hex(&[b]).to_uppercase());
        }
    }
    out
}

pub(crate) fn pct_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let h = s.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(h, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_que_viram_nome_de_arquivo() {
        assert!(check_id("20260924-201224"));
        assert!(check_id("gravacao_2"));
        for ruim in [
            "",
            "../x",
            "a/b",
            "a\\b",
            "com espaço",
            "ação",
            &"x".repeat(65),
        ] {
            assert!(!check_id(ruim), "{ruim}");
        }
    }

    #[test]
    fn hex_e_url_vao_e_voltam() {
        let bytes = [0u8, 1, 0xab, 0xff];
        assert_eq!(hex(&bytes), "0001abff");
        assert_eq!(unhex("0001abff").unwrap(), bytes);
        assert!(unhex("0g").is_none());
        assert!(unhex("abc").is_none());
        let nome = "MARCUS-PC (sala 2) · ação";
        assert_eq!(pct_decode(&pct_encode(nome)).unwrap(), nome);
        assert!(pct_decode("%zz").is_none());
    }

    #[test]
    fn comparacao_em_tempo_constante() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
    }
}
