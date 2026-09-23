//! Reconstrói a Biblioteca a partir dos Markdowns das reuniões.
//!
//! O `.md` em `Documentos\ISPer\Reunioes` é o artefato durável: ele sai antes
//! de qualquer outra coisa (`finish_meeting` grava o arquivo, depois o banco)
//! e sobrevive a qualquer acidente com o SQLite. O banco é um ÍNDICE — útil
//! para buscar, exportar e resumir, mas reconstruível.
//!
//! Este módulo fecha esse ciclo: lê de volta o que [`crate::meeting::render_markdown`]
//! escreveu e devolve os dados para o banco. É o caminho de recuperação quando
//! o índice e a pasta discordam.
//!
//! ## O que volta e o que não volta
//!
//! Volta: título, data, duração, o texto inteiro com horário e falante, o
//! resumo da IA e os momentos marcados.
//!
//! Não volta: a granularidade original. O Markdown guarda PARÁGRAFOS
//! (`group_speech` junta falas seguidas do mesmo falante), então uma reunião
//! reimportada tem menos segmentos, mais longos, que a original. O texto é o
//! mesmo; o fatiamento, não. Os fins de fala são inferidos do início da fala
//! seguinte — o que o leitor vê não muda, mas um SRT reimportado tem blocos
//! mais longos.

use std::path::Path;

/// Uma fala lida do Markdown.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedSegment {
    /// Rótulo do falante como está no arquivo ("Eu", "Participante 1"…).
    pub speaker: String,
    /// Início da fala, em segundos da reunião.
    pub start_secs: f32,
    /// Fim da fala (o início da seguinte, ou o fim da reunião).
    pub end_secs: f32,
    /// Texto da fala.
    pub text: String,
}

/// Uma reunião inteira lida do Markdown.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedMeeting {
    /// Título da reunião (a primeira linha `# ...`).
    pub title: String,
    /// Como estava escrito no arquivo: `dd/mm/aaaa hh:mm`.
    pub started_at: String,
    /// Duração declarada no cabeçalho, em segundos.
    pub duration_secs: f32,
    /// As falas, na ordem do arquivo.
    pub segments: Vec<ImportedSegment>,
    /// O resumo por IA, se o arquivo tiver a seção.
    pub summary: Option<String>,
    /// Momentos marcados, em segundos da reunião.
    pub moments: Vec<f32>,
}

/// Por que um `.md` não pôde ser lido como reunião do ISPer.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// O arquivo não começa com um título (`# ...`).
    #[error("{0}: sem título (a primeira linha deveria ser `# ...`)")]
    NoTitle(String),
    /// Falta a linha de data e duração que o ISPer grava.
    #[error("{0}: sem a linha de data/duração do ISPer")]
    NoHeader(String),
    /// Nenhuma fala no formato do ISPer foi encontrada.
    #[error("{0}: nenhuma fala reconhecida")]
    NoSegments(String),
}

/// Lê um `.md` de reunião.
pub fn parse_markdown(name: &str, md: &str) -> Result<ImportedMeeting, ImportError> {
    let title = md
        .lines()
        .find_map(|l| l.strip_prefix("# "))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| ImportError::NoTitle(name.into()))?;

    // "> Transcrito 100% localmente pelo ISPer em 16/09/2026 11:23 · duração 22:44."
    let header = md
        .lines()
        .find(|l| l.starts_with("> Transcrito") && l.contains(" em "))
        .ok_or_else(|| ImportError::NoHeader(name.into()))?;
    let started_at = header
        .split(" em ")
        .nth(1)
        .and_then(|r| r.split('·').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let duration_secs = header
        .split("duração")
        .nth(1)
        .map(|s| s.trim().trim_end_matches('.'))
        .and_then(parse_ts)
        .unwrap_or(0.0);

    // O resumo fica depois do separador `---`; corta antes de varrer as falas
    // para que um resumo com "**[" dentro não vire fala.
    let (corpo, summary) = split_summary(md);

    let mut segments: Vec<ImportedSegment> = Vec::new();
    for linha in corpo.lines() {
        let Some(seg) = parse_speech_line(linha) else {
            continue;
        };
        segments.push(seg);
    }
    if segments.is_empty() {
        return Err(ImportError::NoSegments(name.into()));
    }
    // O Markdown só guarda o INÍCIO de cada parágrafo. O fim de um é o início
    // do seguinte; o do último, a duração da reunião (ou o próprio início, se
    // a duração vier menor — arquivo estranho não pode gerar fala negativa).
    for i in 0..segments.len() {
        let fim = segments
            .get(i + 1)
            .map(|s| s.start_secs)
            .unwrap_or(duration_secs);
        segments[i].end_secs = fim.max(segments[i].start_secs);
    }

    let moments = parse_moments(corpo);
    Ok(ImportedMeeting {
        title,
        started_at,
        duration_secs,
        segments,
        summary,
        moments,
    })
}

/// Lê o `.md` de um caminho.
pub fn parse_file(path: &Path) -> std::io::Result<Result<ImportedMeeting, ImportError>> {
    let md = std::fs::read_to_string(path)?;
    let nome = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(parse_markdown(&nome, &md))
}

/// `**[00:18] Participante 2:** boleto.` → fala.
fn parse_speech_line(linha: &str) -> Option<ImportedSegment> {
    let resto = linha.strip_prefix("**[")?;
    let (ts, resto) = resto.split_once("] ")?;
    let start_secs = parse_ts(ts)?;
    // O falante vai até `:**`; o texto, depois.
    let (speaker, text) = resto.split_once(":**")?;
    let speaker = speaker.trim();
    if speaker.is_empty() {
        return None;
    }
    Some(ImportedSegment {
        speaker: speaker.to_string(),
        start_secs,
        end_secs: start_secs,
        text: text.trim().to_string(),
    })
}

/// `## Momentos marcados` → os instantes, em segundos.
fn parse_moments(corpo: &str) -> Vec<f32> {
    let Some(secao) = corpo.split("## Momentos marcados").nth(1) else {
        return Vec::new();
    };
    secao
        .lines()
        .take_while(|l| !l.starts_with("## "))
        .filter_map(|l| {
            l.trim()
                .strip_prefix("- **[")?
                .split_once("]**")
                .map(|(t, _)| t)
        })
        .filter_map(parse_ts)
        .collect()
}

/// Separa o corpo do resumo da IA (tudo depois do `---` isolado).
fn split_summary(md: &str) -> (&str, Option<String>) {
    let Some(pos) = md.find("\n---\n") else {
        return (md, None);
    };
    let (corpo, resto) = md.split_at(pos);
    let resumo = resto
        .trim_start_matches("\n---\n")
        .trim()
        // O rodapé é do renderizador, não do resumo.
        .trim_end_matches("> Resumo gerado por IA — revise antes de usar.")
        .trim()
        .to_string();
    (corpo, (!resumo.is_empty()).then_some(resumo))
}

/// `mm:ss` ou `h:mm:ss` → segundos. O inverso de [`crate::meeting::fmt_ts`].
pub fn parse_ts(s: &str) -> Option<f32> {
    let partes: Vec<&str> = s.trim().split(':').collect();
    let n: Vec<u32> = partes
        .iter()
        .map(|p| p.trim().parse().ok())
        .collect::<Option<_>>()?;
    match n[..] {
        [m, s] => Some((m * 60 + s) as f32),
        [h, m, s] => Some((h * 3600 + m * 60 + s) as f32),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting::{SegmentRef, render_markdown};

    fn seg<'a>(speaker: &'a str, a: f32, b: f32, text: &'a str) -> SegmentRef<'a> {
        SegmentRef {
            speaker,
            start_secs: a,
            end_secs: b,
            text,
        }
    }

    #[test]
    fn o_que_o_renderizador_escreve_o_importador_le_de_volta() {
        // A garantia que importa: quem gerou o arquivo e quem o lê não podem
        // divergir. Se o formato do Markdown mudar, este teste quebra.
        let segs = [
            seg("Eu", 0.0, 2.4, "Bom dia, pessoal."),
            seg(
                "Participante 1",
                7.0,
                12.5,
                "O fornecedor A passou de 8 para 12 dias.",
            ),
            seg("Participante 2", 3_720.0, 3_725.5, "Eu confirmo a data."),
        ];
        let md = render_markdown(
            "Planejamento PCP",
            "16/09/2026 11:23",
            3_800.0,
            &segs,
            Some("## Resumo\nRevisão do lead time."),
            &[7.0],
        );
        let m = parse_markdown("teste.md", &md).expect("importa");
        assert_eq!(m.title, "Planejamento PCP");
        assert_eq!(m.started_at, "16/09/2026 11:23");
        assert_eq!(m.duration_secs, 3_800.0);
        assert_eq!(m.segments.len(), 3);
        assert_eq!(m.segments[0].speaker, "Eu");
        assert_eq!(m.segments[0].text, "Bom dia, pessoal.");
        assert_eq!(m.segments[1].speaker, "Participante 1");
        assert_eq!(
            m.segments[2].start_secs, 3_720.0,
            "trecho depois de uma hora"
        );
        assert!(
            m.summary
                .as_deref()
                .is_some_and(|s| s.contains("lead time"))
        );
        assert!(
            !m.summary
                .as_deref()
                .unwrap_or_default()
                .contains("Resumo gerado por IA")
        );
        assert_eq!(m.moments, vec![7.0]);
    }

    #[test]
    fn o_fim_de_cada_fala_vem_do_inicio_da_seguinte() {
        let segs = [
            seg("Eu", 0.0, 2.0, "Primeira."),
            seg("Eu", 30.0, 33.0, "Segunda."),
        ];
        let md = render_markdown("R", "16/09/2026 11:23", 60.0, &segs, None, &[]);
        let m = parse_markdown("r.md", &md).expect("importa");
        assert_eq!(m.segments[0].end_secs, 30.0, "fim = início do próximo");
        assert_eq!(m.segments[1].end_secs, 60.0, "último = duração");
        assert!(m.segments.iter().all(|s| s.end_secs >= s.start_secs));
    }

    #[test]
    fn duracao_menor_que_a_ultima_fala_nao_gera_intervalo_negativo() {
        let md = "# R\n\n> Transcrito 100% localmente pelo ISPer em 16/09/2026 11:23 · duração 00:05.\n\n**[00:40] Eu:** Depois do fim.\n";
        let m = parse_markdown("r.md", md).expect("importa");
        assert_eq!(m.segments[0].start_secs, 40.0);
        assert_eq!(m.segments[0].end_secs, 40.0);
    }

    #[test]
    fn resumo_com_colchetes_nao_vira_fala() {
        let segs = [seg("Eu", 0.0, 2.0, "Oi.")];
        let md = render_markdown(
            "R",
            "16/09/2026 11:23",
            10.0,
            &segs,
            Some("## Resumo\n- **[00:03]** isto é texto do resumo, não fala"),
            &[],
        );
        let m = parse_markdown("r.md", &md).expect("importa");
        assert_eq!(m.segments.len(), 1, "{:#?}", m.segments);
    }

    #[test]
    fn horarios_vao_e_voltam() {
        for s in [0.0f32, 59.0, 60.0, 3_599.0, 3_600.0, 7_265.0] {
            let texto = crate::meeting::fmt_ts(s);
            assert_eq!(parse_ts(&texto), Some(s), "{texto}");
        }
        assert_eq!(parse_ts("xx:yy"), None);
        assert_eq!(parse_ts(""), None);
    }

    #[test]
    fn arquivo_sem_o_que_e_preciso_falha_com_motivo() {
        assert!(matches!(
            parse_markdown("x.md", "sem nada"),
            Err(ImportError::NoTitle(_))
        ));
        assert!(matches!(
            parse_markdown("x.md", "# Título\n\nsem cabeçalho"),
            Err(ImportError::NoHeader(_))
        ));
        assert!(matches!(
            parse_markdown(
                "x.md",
                "# T\n\n> Transcrito 100% localmente pelo ISPer em 1/1/2026 · duração 00:10.\n"
            ),
            Err(ImportError::NoSegments(_))
        ));
    }

    #[test]
    fn falante_renomeado_a_mao_sobrevive() {
        // Quem renomeia "Participante 2" para "Tatiana" na Biblioteca regrava
        // o .md; a reimportação tem que trazer o nome novo.
        let segs = [seg("Tatiana", 0.0, 2.0, "Bom dia.")];
        let md = render_markdown("R", "16/09/2026 11:23", 10.0, &segs, None, &[]);
        let m = parse_markdown("r.md", &md).expect("importa");
        assert_eq!(m.segments[0].speaker, "Tatiana");
    }
}
