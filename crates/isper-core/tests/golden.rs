//! Testes *golden* das exportações: a mesma reunião de exemplo gera Markdown,
//! SRT e DOCX, e a saída é comparada byte a byte com os arquivos em
//! `tests/golden/`. Uma mudança de formato aparece como diff legível no PR —
//! e é aceita conscientemente regenerando os arquivos:
//!
//! ```text
//! ISPER_UPDATE_GOLDEN=1 cargo test --release -p isper-core --test golden
//! ```
//!
//! O DOCX é um ZIP determinístico (data fixa, sem compressão); além do
//! `word/document.xml` (o conteúdo, legível no diff), o teste confere a
//! estrutura do pacote: assinaturas, três entradas, CRC e tamanhos.

use std::path::{Path, PathBuf};

use isper_core::export::{to_docx, to_srt};
use isper_core::meeting::{SegmentRef, render_markdown};

const TITLE: &str = "Planejamento PCP — semana 37";
const STARTED_AT: &str = "09/09/2026 14:00";
const DURATION_SECS: f32 = 1_930.5;
const MOMENTS: [f32; 2] = [131.0, 3.0];
const SUMMARY: &str = "## Resumo\nRevisão do lead time do MRP e do plano de produção da semana.\n\n\
## Pontos principais\n- O lead time do fornecedor A subiu para 12 dias.\n- Ordem 4521 antecipada.\n\n\
## Action items\n- [ ] Atualizar o parâmetro de lead time — Eu\n- [ ] Confirmar a data com a logística — Participante 2\n\n\
## Decisões\n- Manter o estoque de segurança até a revisão de outubro.\n\n\
_Resumo gerado via groq (openai/gpt-oss-120b)._";

/// Reunião de exemplo: dois participantes identificados, falas do mesmo
/// falante que se juntam num parágrafo (pausa < 4 s), pausa longa que separa,
/// fala vazia que é ignorada, caracteres que o XML precisa escapar e um trecho
/// depois de uma hora (formato `h:mm:ss`).
fn sample() -> Vec<SegmentRef<'static>> {
    let seg = |speaker, start, end, text| SegmentRef {
        speaker,
        start_secs: start,
        end_secs: end,
        text,
    };
    vec![
        seg("Eu", 0.0, 2.4, "Bom dia, pessoal."),
        seg(
            "Eu",
            2.9,
            6.1,
            "Vamos revisar o lead time do MRP & o plano da semana.",
        ),
        seg(
            "Participante 1",
            7.0,
            12.5,
            "O fornecedor A passou de 8 para 12 dias.",
        ),
        seg("Participante 1", 13.0, 13.2, "   "),
        seg(
            "Participante 2",
            14.0,
            19.8,
            "Então a ordem 4521 precisa ser antecipada, certo?",
        ),
        seg("Eu", 20.5, 24.0, "Certo — eu atualizo o parâmetro hoje."),
        seg(
            "Participante 1",
            130.0,
            136.0,
            "Sugiro manter o estoque de segurança até outubro.",
        ),
        seg("Eu", 136.5, 138.0, "Combinado."),
        seg(
            "Participante 2",
            3_720.0,
            3_725.5,
            "Eu confirmo a data com a logística <amanhã>.",
        ),
    ]
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
}

/// Compara `actual` com o arquivo `name` (ou o regrava com `ISPER_UPDATE_GOLDEN=1`).
fn check(name: &str, actual: &[u8]) {
    let path = golden_dir().join(name);
    if std::env::var_os("ISPER_UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, actual).expect("regravar o arquivo golden");
        return;
    }
    let expected = std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "sem arquivo golden {} ({e}) — gere com ISPER_UPDATE_GOLDEN=1",
            path.display()
        )
    });
    if expected != actual {
        let (exp, act) = (
            String::from_utf8_lossy(&expected),
            String::from_utf8_lossy(actual),
        );
        let first_diff = exp
            .lines()
            .zip(act.lines())
            .position(|(a, b)| a != b)
            .map(|i| i + 1)
            .unwrap_or_else(|| exp.lines().count().min(act.lines().count()) + 1);
        panic!(
            "{name} mudou (primeira linha diferente: {first_diff}).\n\
             Se a mudança é intencional, regenere com ISPER_UPDATE_GOLDEN=1.\n\
             --- esperado ---\n{exp}\n--- obtido ---\n{act}"
        );
    }
}

#[test]
fn markdown_da_reuniao_bate_com_o_golden() {
    let md = render_markdown(
        TITLE,
        STARTED_AT,
        DURATION_SECS,
        &sample(),
        Some(SUMMARY),
        &MOMENTS,
    );
    check("reuniao.md", md.as_bytes());
}

#[test]
fn markdown_sem_resumo_nem_momentos_bate_com_o_golden() {
    let md = render_markdown(TITLE, STARTED_AT, DURATION_SECS, &sample(), None, &[]);
    check("reuniao-sem-resumo.md", md.as_bytes());
}

#[test]
fn srt_da_reuniao_bate_com_o_golden() {
    check("reuniao.srt", to_srt(&sample()).as_bytes());
}

#[test]
fn docx_da_reuniao_bate_com_o_golden() {
    let docx = to_docx(
        TITLE,
        STARTED_AT,
        DURATION_SECS,
        &sample(),
        Some(SUMMARY),
        &MOMENTS,
    );
    let entries = unzip_store(&docx);
    assert_eq!(
        entries.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
        ["[Content_Types].xml", "_rels/.rels", "word/document.xml"]
    );
    let document = &entries[2].1;
    check("reuniao-document.xml", document);
    // O pacote inteiro também é determinístico: mesmo conteúdo, mesmos bytes.
    check("reuniao.docx", &docx);
}

/// Lê um ZIP "store" mínimo (o formato que `to_docx` escreve) conferindo as
/// assinaturas, o CRC-32 e os tamanhos de cada entrada — um leitor de verdade
/// (Word, `Expand-Archive`) faria as mesmas checagens.
fn unzip_store(zip: &[u8]) -> Vec<(String, Vec<u8>)> {
    let u16_at = |i: usize| u16::from_le_bytes([zip[i], zip[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([zip[i], zip[i + 1], zip[i + 2], zip[i + 3]]);

    // EOCD nos últimos 22 bytes (sem comentário).
    let eocd = zip.len() - 22;
    assert_eq!(u32_at(eocd), 0x0605_4b50, "assinatura do EOCD");
    let count = u16_at(eocd + 10) as usize;
    let cd_size = u32_at(eocd + 12) as usize;
    let cd_offset = u32_at(eocd + 16) as usize;
    assert_eq!(
        cd_offset + cd_size,
        eocd,
        "diretório central encosta no EOCD"
    );

    let mut entries = Vec::with_capacity(count);
    let mut pos = cd_offset;
    for _ in 0..count {
        assert_eq!(u32_at(pos), 0x0201_4b50, "assinatura da entrada central");
        assert_eq!(u16_at(pos + 10), 0, "método store");
        let crc = u32_at(pos + 16);
        let size = u32_at(pos + 20) as usize;
        assert_eq!(
            u32_at(pos + 24) as usize,
            size,
            "tamanho comprimido = original"
        );
        let name_len = u16_at(pos + 28) as usize;
        let local = u32_at(pos + 42) as usize;
        let name = String::from_utf8(zip[pos + 46..pos + 46 + name_len].to_vec())
            .expect("nome da entrada em UTF-8");
        pos += 46 + name_len;

        assert_eq!(u32_at(local), 0x0403_4b50, "assinatura do cabeçalho local");
        assert_eq!(u32_at(local + 14), crc, "CRC igual nos dois cabeçalhos");
        let local_name_len = u16_at(local + 26) as usize;
        let extra_len = u16_at(local + 28) as usize;
        let data_start = local + 30 + local_name_len + extra_len;
        let data = zip[data_start..data_start + size].to_vec();
        assert_eq!(crc32fast::hash(&data), crc, "CRC-32 de {name}");
        entries.push((name, data));
    }
    entries
}
