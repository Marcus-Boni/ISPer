//! Exportações da reunião a partir dos segmentos guardados: SRT (legendas) e
//! DOCX (Word). O DOCX é um pacote OOXML mínimo — três XMLs dentro de um ZIP
//! sem compressão, escrito aqui mesmo (formato estável e pequeno; não vale
//! uma dependência inteira de ZIP só para isso).

use crate::meeting::{SegmentRef, fmt_ts, group_speech, moment_excerpts};

/// Legendas SRT: uma entrada por segmento, com o falante na frente do texto.
pub fn to_srt(segments: &[SegmentRef<'_>]) -> String {
    let mut out = String::new();
    let mut n = 0u32;
    for s in segments {
        let text = s.text.trim();
        if text.is_empty() {
            continue;
        }
        n += 1;
        let end = s.end_secs.max(s.start_secs + 0.5);
        out.push_str(&format!(
            "{n}\n{} --> {}\n{}: {}\n\n",
            srt_time(s.start_secs),
            srt_time(end),
            s.speaker,
            text
        ));
    }
    out
}

fn srt_time(secs: f32) -> String {
    let ms = (secs.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        (ms / 60_000) % 60,
        (ms / 1000) % 60,
        ms % 1000
    )
}

/// Documento Word com título, cabeçalho, resumo (se houver) e um parágrafo
/// por grupo de falas — o mesmo agrupamento do Markdown e da Biblioteca.
pub fn to_docx(
    title: &str,
    started_at: &str,
    duration_secs: f32,
    segments: &[SegmentRef<'_>],
    summary: Option<&str>,
    moments: &[f32],
) -> Vec<u8> {
    let mut body = String::new();
    body.push_str(&para(&[run(title, true, 36)]));
    body.push_str(&para(&[run(
        &format!(
            "Transcrito 100% localmente pelo ISPer em {started_at} · duração {}",
            fmt_ts(duration_secs)
        ),
        false,
        18,
    )]));
    if let Some(summary) = summary.map(str::trim).filter(|s| !s.is_empty()) {
        body.push_str(&para(&[run("Resumo (IA)", true, 26)]));
        for line in summary.lines() {
            let line = line.trim_start_matches('#').trim();
            body.push_str(&para(&[run(line, false, 22)]));
        }
        body.push_str(&para(&[run(
            "Resumo gerado por IA — revise antes de usar.",
            false,
            18,
        )]));
    }
    if !moments.is_empty() {
        body.push_str(&para(&[run("Momentos marcados", true, 26)]));
        for (at, excerpt) in moment_excerpts(segments, moments) {
            body.push_str(&para(&[
                run(&format!("★ [{}] ", fmt_ts(at)), true, 22),
                run(&excerpt, false, 22),
            ]));
        }
    }
    body.push_str(&para(&[run("Transcript", true, 26)]));
    for g in group_speech(segments.iter().copied()) {
        body.push_str(&para(&[
            run(
                &format!("[{}] {}: ", fmt_ts(g.start_secs), g.speaker),
                true,
                22,
            ),
            run(&g.text, false, 22),
        ]));
    }
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
<w:body>{body}<w:sectPr/></w:body></w:document>"
    );
    zip_store(&[
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ("_rels/.rels", RELS.as_bytes()),
        ("word/document.xml", document.as_bytes()),
    ])
}

const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
</Types>";

const RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
</Relationships>";

/// Um trecho de texto com formatação (`half_points` = tamanho da fonte × 2).
fn run(text: &str, bold: bool, half_points: u32) -> String {
    format!(
        "<w:r><w:rPr>{}<w:sz w:val=\"{half_points}\"/></w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r>",
        if bold { "<w:b/>" } else { "" },
        xml_escape(text)
    )
}

fn para(runs: &[String]) -> String {
    format!("<w:p>{}</w:p>", runs.concat())
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Controles (exceto tab/quebras) não são XML válido.
            c if c.is_control() && c != '\t' && c != '\n' && c != '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// ZIP com método "store" (sem compressão): cabeçalhos locais, diretório
/// central e EOCD. Data fixa (1980-01-01) para saída determinística.
fn zip_store(entries: &[(&str, &[u8])]) -> Vec<u8> {
    const DOS_TIME: u16 = 0;
    const DOS_DATE: u16 = 0x0021;
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    for (name, data) in entries {
        let crc = crc32fast::hash(data);
        let offset = out.len() as u32;
        let size = data.len() as u32;
        let name_len = name.len() as u16;

        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes()); // local file header
        out.extend_from_slice(&20u16.to_le_bytes()); // versão mínima
        out.extend_from_slice(&0x0800u16.to_le_bytes()); // nomes em UTF-8
        out.extend_from_slice(&0u16.to_le_bytes()); // método: store
        out.extend_from_slice(&DOS_TIME.to_le_bytes());
        out.extend_from_slice(&DOS_DATE.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // sem campo extra
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes()); // entrada do diretório
        central.extend_from_slice(&20u16.to_le_bytes()); // feito por
        central.extend_from_slice(&20u16.to_le_bytes()); // versão mínima
        central.extend_from_slice(&0x0800u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&DOS_TIME.to_le_bytes());
        central.extend_from_slice(&DOS_DATE.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&name_len.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra
        central.extend_from_slice(&0u16.to_le_bytes()); // comentário
        central.extend_from_slice(&0u16.to_le_bytes()); // disco
        central.extend_from_slice(&0u16.to_le_bytes()); // atributos internos
        central.extend_from_slice(&0u32.to_le_bytes()); // atributos externos
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let cd_offset = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend_from_slice(&central);
    let count = entries.len() as u16;
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes()); // EOCD
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(speaker: &'static str, start: f32, end: f32, text: &'static str) -> SegmentRef<'static> {
        SegmentRef {
            speaker,
            start_secs: start,
            end_secs: end,
            text,
        }
    }

    #[test]
    fn srt_numera_e_formata_tempos() {
        let srt = to_srt(&[
            seg("Eu", 0.0, 1.25, "Olá."),
            seg("Participantes", 3661.5, 3662.0, "Oi"),
        ]);
        assert!(srt.starts_with("1\n00:00:00,000 --> 00:00:01,250\nEu: Olá.\n\n"));
        assert!(srt.contains("2\n01:01:01,500 --> 01:01:02,000\nParticipantes: Oi\n\n"));
    }

    #[test]
    fn srt_garante_duracao_minima() {
        let srt = to_srt(&[seg("Eu", 10.0, 10.0, "curto")]);
        assert!(srt.contains("00:00:10,000 --> 00:00:10,500"));
    }

    #[test]
    fn xml_escapa_e_remove_controles() {
        assert_eq!(
            xml_escape("a < b & c > \"d\" \u{1}e"),
            "a &lt; b &amp; c &gt; &quot;d&quot; e"
        );
    }

    #[test]
    fn docx_e_um_zip_valido_com_document_xml() {
        let docx = to_docx(
            "Título & teste",
            "09/09/2026",
            61.0,
            &[seg("Eu", 0.0, 1.0, "Olá <mundo>")],
            Some("Resumo."),
            &[0.5],
        );
        assert_eq!(&docx[..4], &[0x50, 0x4b, 0x03, 0x04]);
        let text = String::from_utf8_lossy(&docx);
        assert!(text.contains("word/document.xml"));
        assert!(text.contains("T&iacute;tulo &amp; teste") || text.contains("Título &amp; teste"));
        assert!(text.contains("Olá &lt;mundo&gt;"));
        assert!(text.contains("Momentos marcados"));
        assert!(text.contains("★ [00:00] "));
        // EOCD no fim, com 3 entradas.
        let n = docx.len();
        assert_eq!(&docx[n - 22..n - 18], &[0x50, 0x4b, 0x05, 0x06]);
        assert_eq!(u16::from_le_bytes([docx[n - 12], docx[n - 11]]), 3);
    }
}
