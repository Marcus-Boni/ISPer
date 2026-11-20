//! O que a janela de chamada do Microsoft Teams diz, pela árvore de
//! acessibilidade, sobre quem está na chamada e quem está falando.
//!
//! Cada regra falha para o lado de não ler nada: um Teams que mudou o texto
//! dos rótulos dá "Participante N", nunca um nome errado. Os sinais lidos:
//!
//! - **Painel Pessoas** (a lista "Participantes"/"Attendees"/"Nesta reunião"):
//!   cada linha diz "Nome, Papel, Estado do microfone" ("Ana Souza,
//!   Organizador, Sem mudo"). É a lista de presença. Algumas versões
//!   acrescentam "Falando" na linha de quem fala.
//! - **Marca explícita de quem fala**, em qualquer lugar fora do chat e das
//!   legendas: "Ana Souza, falando", "Ana Souza está falando", "Orador ativo:
//!   Ana Souza" — e as mesmas formas em inglês.
//! - **"(Você)"/"(You)"** depois do nome de quem usa o ISPer.
//!
//! Um bloco de vídeo com "Ana Souza, Sem mudo" NÃO conta como presença: a
//! galeria mostra só parte das pessoas, e uma chamada de cinco vista como de
//! duas daria nome errado. Presença vem só do painel Pessoas.
//!
//! O vocabulário em inglês segue o que o Yogurt mediu no Teams 2.28 (o
//! anel de quem fala não vira texto na árvore; a lista de presença sim). O
//! português é o do Teams em pt-BR, a confirmar com uma chamada real pelo
//! `isper-cli teams-probe` — até lá, uma versão que escreve diferente fica
//! com os nomes genéricos.

use serde::{Deserialize, Serialize};

use super::tree::{UiNode, UiWindow};

/// O que uma leitura da chamada mostrou.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallReading {
    /// Quem o Teams marca falando agora, cada um uma vez.
    #[serde(default)]
    pub speaking: Vec<String>,
    /// Uma entrada por linha do painel Pessoas. Dois nomes iguais são duas
    /// pessoas homônimas, não a mesma linha lida duas vezes.
    #[serde(default)]
    pub participants: Vec<String>,
    /// Quem usa o ISPer, quando o Teams diz ("(Você)").
    #[serde(default)]
    pub me: Option<String>,
}

// ------------------------------------------------------------ vocabulário
// Tudo em minúsculas; a comparação é sem diferenciar caixa.

/// Estado do microfone ou da câmera depois do nome.
const STATUS: &[&str] = &[
    "muted",
    "unmuted",
    "mic off",
    "mic on",
    "mic muted",
    "mic unmuted",
    "microphone off",
    "microphone on",
    "microphone muted",
    "microphone unmuted",
    "video off",
    "video on",
    "video is off",
    "video is on",
    "camera off",
    "camera on",
    "camera is off",
    "camera is on",
    "mudo",
    "com mudo",
    "sem mudo",
    "mudo ativado",
    "mudo desativado",
    "microfone desativado",
    "microfone ativado",
    "microfone desligado",
    "microfone ligado",
    "microfone silenciado",
    "vídeo desativado",
    "vídeo ativado",
    "vídeo desligado",
    "vídeo ligado",
    "câmera desativada",
    "câmera ativada",
    "câmera desligada",
    "câmera ligada",
];

/// Papéis que o Teams põe entre o nome e o estado.
const ROLES: &[&str] = &[
    "organizer",
    "co-organizer",
    "coorganizer",
    "presenter",
    "attendee",
    "guest",
    "external",
    "host",
    "co-host",
    "interpreter",
    "unverified",
    "organizador",
    "organizadora",
    "coorganizador",
    "coorganizadora",
    "apresentador",
    "apresentadora",
    "participante",
    "convidado",
    "convidada",
    "externo",
    "externa",
    "anfitrião",
    "intérprete",
    "não verificado",
    "não verificada",
];

/// Outros estados que dizem nada sobre quem é a pessoa.
const OTHER_STATUS: &[&str] = &[
    "hand raised",
    "raised hand",
    "pinned",
    "spotlighted",
    "presenting",
    "sharing",
    "screen sharing",
    "mão levantada",
    "mão erguida",
    "fixado",
    "fixada",
    "em destaque",
    "apresentando",
    "compartilhando",
    "compartilhando a tela",
];

/// Um campo que, sozinho, marca quem fala ("Ana Souza, Falando").
const SPEAKING_FIELDS: &[&str] = &[
    "speaking",
    "talking",
    "active speaker",
    "now speaking",
    "speaking now",
    "falando",
    "falando agora",
    "orador ativo",
    "oradora ativa",
    "falante ativo",
    "falante ativa",
];

/// Prefixos que dizem quem fala ("Orador ativo: Ana Souza"); os mais longos
/// primeiro, para "falando agora" não ser lido como "falando" + "agora…".
const SPEAKING_PREFIXES: &[&str] = &[
    "active speaker",
    "now speaking",
    "speaking now",
    "speaking",
    "orador ativo",
    "oradora ativa",
    "falante ativo",
    "falante ativa",
    "falando agora",
    "falando",
];

/// Sufixos que dizem quem fala ("Ana Souza está falando").
const SPEAKING_SUFFIXES: &[&str] = &[
    " is speaking",
    " is talking",
    " está falando",
    " esta falando",
];

/// Marcas, entre parênteses, de quem usa o app.
const ME_TAGS: &[&str] = &["you", "me", "você", "voce", "eu"];

/// Nome da lista que é o painel Pessoas (sozinho ou com a contagem: "Na
/// reunião (3)").
const ROSTER_NAMES: &[&str] = &[
    "participants",
    "attendees",
    "people",
    "in this meeting",
    "in the meeting",
    "in this call",
    "meeting participants",
    "participantes",
    "pessoas",
    "nesta reunião",
    "na reunião",
    "nesta chamada",
    "na chamada",
    "participantes da reunião",
];

/// Papéis que podem conter o painel Pessoas.
const ROSTER_CONTAINERS: &[&str] = &[
    "tree",
    "tree item",
    "table",
    "data grid",
    "list",
    "group",
    "pane",
];

/// Papéis de uma linha do painel.
const ROWS: &[&str] = &["tree item", "list item", "data item"];

/// Pedaços da janela onde há texto de pessoas, mas não sinal de quem fala:
/// o chat (alguém pode escrever "Ana está falando") e as legendas.
const CHAT_WORDS: &[&str] = &[
    "chat",
    "conversa",
    "mensagens",
    "messages",
    "captions",
    "legendas",
    "transcript",
    "transcrição",
];

/// Botões que só existem numa janela de chamada: sair…
const LEAVE_BUTTONS: &[&str] = &[
    "leave",
    "leave call",
    "leave meeting",
    "hang up",
    "end call",
    "end meeting",
    "sair",
    "sair da reunião",
    "sair da chamada",
    "desligar",
    "desligar chamada",
    "encerrar chamada",
    "encerrar reunião",
];

/// …e o microfone (prefixos: "Ativar mudo (Ctrl+Shift+M)").
const MIC_BUTTONS: &[&str] = &[
    "mute",
    "unmute",
    "mic",
    "microphone",
    "turn mic",
    "turn on mic",
    "turn off mic",
    "ativar mudo",
    "desativar mudo",
    "mudo",
    "microfone",
    "ativar microfone",
    "desativar microfone",
    "silenciar",
    "ligar microfone",
    "desligar microfone",
];

const BUTTONS: &[&str] = &["button", "split button"];

// ------------------------------------------------------------ texto

/// Espaços colapsados, espaço não separável comum, pontas limpas.
fn squash(s: &str) -> String {
    s.replace('\u{a0}', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A chave de comparação de um nome: sem caixa e com espaços colapsados.
pub fn name_key(name: &str) -> String {
    squash(name).to_lowercase()
}

fn is_in(list: &[&str], s: &str) -> bool {
    let low = s.trim().to_lowercase();
    list.contains(&low.as_str())
}

/// `s` sem o prefixo `p`, comparado sem caixa. `p` é ASCII (todos os
/// prefixos da lista são), então o corte nunca parte um caractere.
fn strip_prefix_ci<'a>(s: &'a str, p: &str) -> Option<&'a str> {
    debug_assert!(p.is_ascii());
    s.get(..p.len())
        .filter(|head| head.eq_ignore_ascii_case(p))
        .map(|_| &s[p.len()..])
}

/// `s` sem o sufixo `suf`, comparado sem caixa; `None` se o corte cair no
/// meio de um caractere.
fn strip_suffix_ci<'a>(s: &'a str, suf: &str) -> Option<&'a str> {
    let cut = s.len().checked_sub(suf.len())?;
    let tail = s.get(cut..)?;
    (tail.to_lowercase() == suf).then(|| &s[..cut])
}

/// `s` é a palavra `w`, ou começa/termina com ela como palavra inteira.
fn has_word_at_edge(low: &str, w: &str) -> bool {
    low == w
        || low
            .strip_prefix(w)
            .is_some_and(|rest| rest.starts_with(' '))
        || low.strip_suffix(w).is_some_and(|rest| rest.ends_with(' '))
}

/// Tira atalhos entre parênteses do fim de um rótulo de botão: "Sair
/// (Ctrl+Shift+H)" → "Sair".
fn without_shortcut(label: &str) -> &str {
    let t = label.trim();
    if let Some(open) = t.rfind('(')
        && t.ends_with(')')
    {
        let inner = &t[open + 1..t.len() - 1];
        if inner.contains('+') {
            return t[..open].trim_end();
        }
    }
    t
}

/// Uma palavra do próprio Teams (estado, papel, rótulo de lista): nunca é
/// nome de ninguém.
fn app_word(s: &str) -> bool {
    [
        STATUS,
        ROLES,
        OTHER_STATUS,
        SPEAKING_FIELDS,
        ROSTER_NAMES,
        CHAT_WORDS,
        LEAVE_BUTTONS,
        MIC_BUTTONS,
        ME_TAGS,
    ]
    .iter()
    .any(|l| is_in(l, s))
        || is_in(
            &[
                "microsoft teams",
                "teams",
                "reunião",
                "meeting",
                "chamada",
                "call",
            ],
            s,
        )
}

/// O que um rótulo diz de uma pessoa.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Persona {
    name: String,
    /// O rótulo marca a pessoa falando.
    speaking: bool,
    /// O rótulo traz estado de microfone/câmera (é uma linha de presença).
    status: bool,
    /// O rótulo traz "(Você)".
    me: bool,
}

/// Tira as marcas entre parênteses do fim de um nome ("Ana (Organizador)
/// (Você)" → "Ana") e diz se uma delas era "(Você)".
fn strip_tags(name: &str) -> (String, bool) {
    let mut s = name.trim().to_string();
    let mut me = false;
    loop {
        let t = s.trim_end();
        if !t.ends_with(')') {
            break;
        }
        let Some(open) = t.rfind('(') else { break };
        let inner = t[open + 1..t.len() - 1].trim();
        let tags: Vec<&str> = inner.split(',').map(str::trim).collect();
        let known = tags
            .iter()
            .all(|tag| is_in(ME_TAGS, tag) || is_in(ROLES, tag));
        if !known {
            break;
        }
        me |= tags.iter().any(|tag| is_in(ME_TAGS, tag));
        s = t[..open].trim_end().to_string();
    }
    (s, me)
}

/// Um nome de verdade: 2 a 60 caracteres, com letra, sem cara de hora, link
/// ou palavra do próprio Teams.
fn plausible_name(name: &str) -> bool {
    let n = name.chars().count();
    (2..=60).contains(&n)
        && name.chars().any(char::is_alphabetic)
        && !name.contains(':')
        && !name.contains('/')
        && !name.to_lowercase().contains("http")
        && !app_word(name)
}

fn persona(raw_name: &str, speaking: bool, status: bool) -> Option<Persona> {
    let (name, me) = strip_tags(raw_name);
    let name = squash(name.trim_matches(|c: char| matches!(c, ',' | '.' | ';' | ':')));
    plausible_name(&name).then_some(Persona {
        name,
        speaking,
        status,
        me,
    })
}

/// Lê um rótulo. `None` quando ele não diz nada de uma pessoa.
fn parse_label(raw: &str) -> Option<Persona> {
    let label = squash(raw);
    if label.is_empty() {
        return None;
    }

    // "Orador ativo: Ana Souza", "Active speaker, Ana Souza"
    for p in SPEAKING_PREFIXES {
        if let Some(rest) = strip_prefix_ci(&label, p)
            && let Some(name) = rest.strip_prefix(':').or_else(|| rest.strip_prefix(','))
        {
            return persona(name, true, false);
        }
    }
    // "Ana Souza está falando", "Ana Souza is speaking"
    for s in SPEAKING_SUFFIXES {
        if let Some(name) = strip_suffix_ci(&label, s) {
            return persona(name, true, false);
        }
    }
    // "Ana Souza, Organizador, Sem mudo, Falando": do fim para o começo, os
    // campos conhecidos saem; o que sobra é o nome. Sem nenhum campo de
    // estado ou de fala, não é rótulo de pessoa.
    let fields: Vec<&str> = label.split(',').map(str::trim).collect();
    let (mut speaking, mut status) = (false, false);
    let mut end = fields.len();
    while end > 1 {
        let f = fields[end - 1];
        if is_in(SPEAKING_FIELDS, f) {
            speaking = true;
        } else if is_in(STATUS, f) {
            status = true;
        } else if !(is_in(ROLES, f) || is_in(OTHER_STATUS, f) || f.is_empty()) {
            break;
        }
        end -= 1;
    }
    if !(speaking || status) {
        // Sem estado, só "Ana Souza (Você)" ainda diz algo: quem é você.
        let (name, me) = strip_tags(&label);
        return (me && fields.len() == 1)
            .then(|| persona(&name, false, false).map(|p| Persona { me: true, ..p }))
            .flatten();
    }
    persona(&fields[..end].join(", "), speaking, status)
}

fn is_row(n: &UiNode) -> bool {
    ROWS.contains(&n.role.as_str())
}

/// O nome da lista é o do painel Pessoas, sozinho ou com a contagem.
fn roster_name(name: &str) -> bool {
    let n = squash(name).to_lowercase();
    let base = match n.rfind('(') {
        Some(open)
            if n.ends_with(')') && n[open + 1..n.len() - 1].trim().parse::<u32>().is_ok() =>
        {
            n[..open].trim_end()
        }
        _ => n.as_str(),
    };
    ROSTER_NAMES.contains(&base)
}

fn is_roster_container(n: &UiNode) -> bool {
    ROSTER_CONTAINERS.contains(&n.role.as_str()) && n.texts().any(roster_name)
}

/// O elemento é o chat ou as legendas ("Chat da reunião", "Legendas ao
/// vivo"): nada dentro dele é sinal de quem fala. A janela e o documento
/// ficam de fora — o nome deles é o título da reunião, que é do usuário.
fn is_chat_container(n: &UiNode) -> bool {
    !matches!(n.role.as_str(), "window" | "document" | "title bar")
        && n.texts().any(|t| {
            let low = squash(t).to_lowercase();
            CHAT_WORDS.iter().any(|w| has_word_at_edge(&low, w))
        })
}

/// Os textos de uma linha: os dela e os dos descendentes que não são outra
/// linha (o cabeçalho "Nesta reunião (3)" contém as linhas das pessoas, e
/// não pode virar uma pessoa a mais).
fn row_texts(row: &UiNode) -> Vec<String> {
    fn collect(n: &UiNode, out: &mut Vec<String>, top: bool) {
        if !top && is_row(n) {
            return;
        }
        out.extend(n.texts().map(squash));
        for k in &n.children {
            collect(k, out, false);
        }
    }
    let mut out = Vec::new();
    collect(row, &mut out, true);
    out
}

/// O que uma linha do painel Pessoas diz: o primeiro rótulo que é de pessoa;
/// se nenhum sozinho for, os textos da linha juntos ("Ana Souza" + "Sem
/// mudo" em elementos separados).
fn read_row(row: &UiNode) -> Option<Persona> {
    let texts = row_texts(row);
    let single = texts
        .iter()
        .filter_map(|t| parse_label(t))
        .find(|p| p.status || p.speaking || p.me);
    single.or_else(|| {
        let joined = texts.join(", ");
        (texts.len() > 1).then(|| parse_label(&joined)).flatten()
    })
}

fn push_unique(list: &mut Vec<String>, name: &str) {
    let key = name_key(name);
    if !list.iter().any(|x| name_key(x) == key) {
        list.push(name.to_string());
    }
}

/// O que as janelas de chamada do Teams mostram agora.
pub fn read_call(windows: &[UiWindow]) -> CallReading {
    /// Desce a árvore sabendo se está no painel Pessoas e dentro de uma
    /// linha dele; o chat e as legendas são pulados inteiros.
    fn visit(n: &UiNode, in_roster: bool, in_row: bool, out: &mut CallReading) {
        if is_chat_container(n) {
            return;
        }
        let in_roster = in_roster || is_roster_container(n);
        if in_roster && is_row(n) {
            if let Some(p) = read_row(n) {
                out.participants.push(p.name.clone());
                if p.speaking {
                    push_unique(&mut out.speaking, &p.name);
                }
                if p.me {
                    out.me = Some(p.name);
                }
            }
            // Só as linhas de dentro (o cabeçalho "Nesta reunião (3)"
            // contém as pessoas); o resto já foi lido pela linha.
            for k in &n.children {
                visit(k, true, true, out);
            }
            return;
        }
        // Fora do painel, só a marca explícita de quem fala. Um campo de
        // edição é o que alguém digita: nunca.
        if !in_row && n.role != "edit" {
            for t in n.texts() {
                if let Some(p) = parse_label(t).filter(|p| p.speaking) {
                    push_unique(&mut out.speaking, &p.name);
                }
            }
        }
        for k in &n.children {
            visit(k, in_roster, in_row, out);
        }
    }
    let mut out = CallReading::default();
    for w in windows {
        visit(&w.tree, false, false, &mut out);
    }
    out
}

fn button_label(n: &UiNode) -> Option<String> {
    BUTTONS
        .contains(&n.role.as_str())
        .then(|| {
            n.name
                .as_deref()
                .map(|t| squash(without_shortcut(t)).to_lowercase())
        })
        .flatten()
}

/// A janela é de uma chamada: tem o botão de sair e o do microfone. A
/// janela principal do Teams (chat, calendário) não tem, e o texto dela
/// nunca é lido.
pub fn is_call_window(tree: &UiNode) -> bool {
    let (mut leave, mut mic) = (false, false);
    tree.walk(&mut |n, _| {
        if let Some(label) = button_label(n) {
            leave |= LEAVE_BUTTONS.contains(&label.as_str());
            mic |= MIC_BUTTONS
                .iter()
                .any(|m| label == *m || label.starts_with(&format!("{m} ")));
        }
    });
    leave && mic
}

/// Uma janela em números, para o log: nada de nome, título ou texto.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TreeSummary {
    /// Janelas lidas.
    pub windows: usize,
    /// Elementos no total.
    pub nodes: usize,
    /// Linhas do painel Pessoas reconhecidas.
    pub roster_rows: usize,
    /// Pessoas marcadas falando.
    pub speaking: usize,
    /// O próprio usuário foi reconhecido ("(Você)").
    pub me_known: bool,
}

/// O resumo de uma leitura para o log (contagens apenas).
pub fn summarize(windows: &[UiWindow]) -> TreeSummary {
    let r = read_call(windows);
    TreeSummary {
        windows: windows.len(),
        nodes: windows.iter().map(|w| w.tree.count()).sum(),
        roster_rows: r.participants.len(),
        speaking: r.speaking.len(),
        me_known: r.me.is_some(),
    }
}

/// Uma cópia da árvore sem dado pessoal, para virar fixture de teste:
/// nomes trocados por "Pessoa A", "Pessoa B"… (a mesma pessoa, a mesma
/// letra, em toda a árvore), o vocabulário do Teams mantido, e qualquer
/// outro texto (título, chat, legenda) trocado pelo tamanho dele.
pub fn anonymize(windows: &[UiWindow]) -> Vec<UiWindow> {
    // Os nomes que a leitura reconhece (inclusive o de uma linha montada a
    // partir de vários elementos) e os de qualquer rótulo de pessoa. Em
    // ordem de aparição, para o mesmo nome ganhar sempre a mesma letra.
    let mut names: Vec<String> = Vec::new();
    let reading = read_call(windows);
    for n in reading
        .participants
        .iter()
        .chain(&reading.speaking)
        .chain(reading.me.as_ref())
    {
        push_unique(&mut names, n);
    }
    for w in windows {
        w.tree.walk(&mut |n, _| {
            for t in n.texts() {
                if let Some(p) = parse_label(t) {
                    push_unique(&mut names, &p.name);
                }
            }
        });
    }
    let alias = |i: usize| {
        let letter = (b'A' + (i % 26) as u8) as char;
        match i / 26 {
            0 => format!("Pessoa {letter}"),
            round => format!("Pessoa {letter}{round}"),
        }
    };
    let aliases: Vec<String> = (0..names.len()).map(alias).collect();
    // Os mais longos primeiro: "Ana Souza Lima" antes de "Ana Souza".
    let mut by_len: Vec<(&String, &String)> = names.iter().zip(&aliases).collect();
    by_len.sort_by_key(|(n, _)| std::cmp::Reverse(n.len()));
    let safe = |text: &str| -> Option<String> {
        let mut out = squash(text);
        for (name, alias) in &by_len {
            out = out.replace(name.as_str(), alias);
        }
        let known_alias = |f: &str| aliases.iter().any(|a| a == f.trim());
        let all_known = without_shortcut(&out).split(',').all(|f| {
            let f = f.trim();
            let (bare, _) = strip_tags(f);
            let low = f.to_lowercase();
            let speaking_prefix = SPEAKING_PREFIXES.iter().any(|p| {
                strip_prefix_ci(f, p)
                    .and_then(|rest| rest.strip_prefix(':'))
                    .is_some_and(known_alias)
            });
            let speaking_suffix = SPEAKING_SUFFIXES
                .iter()
                .any(|s| strip_suffix_ci(f, s).is_some_and(known_alias));
            let mic = MIC_BUTTONS
                .iter()
                .any(|m| low == *m || low.starts_with(&format!("{m} ")));
            f.is_empty()
                || known_alias(&bare)
                || app_word(f)
                || roster_name(f)
                || speaking_prefix
                || speaking_suffix
                || mic
        });
        all_known.then_some(out)
    };
    fn redact_id(id: &str) -> String {
        let digits = id.chars().filter(char::is_ascii_hexdigit).count();
        if id.contains(':') || id.contains('@') || digits >= 8 {
            "‹id›".into()
        } else {
            id.to_string()
        }
    }
    fn copy(n: &UiNode, safe: &dyn Fn(&str) -> Option<String>) -> UiNode {
        let text = |t: &Option<String>| {
            t.as_deref()
                .map(|t| safe(t).unwrap_or_else(|| format!("‹{}›", t.chars().count())))
        };
        UiNode {
            role: n.role.clone(),
            name: text(&n.name),
            help: text(&n.help),
            automation_id: n.automation_id.as_deref().map(redact_id),
            classes: n.classes.clone(),
            children: n.children.iter().map(|k| copy(k, safe)).collect(),
        }
    }
    windows
        .iter()
        .map(|w| UiWindow {
            title: safe(&w.title).unwrap_or_else(|| "‹título›".into()),
            tree: copy(&w.tree, &safe),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<UiWindow> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/teams")
            .join(name);
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path:?}: {e}"))
    }

    #[test]
    fn rotulos_de_pessoa_nos_dois_idiomas() {
        let p = parse_label("Ana Souza, Organizador, Sem mudo").unwrap();
        assert_eq!(
            (p.name.as_str(), p.status, p.speaking),
            ("Ana Souza", true, false)
        );
        let p = parse_label("Dana Smith, Organizer, Unmuted, Speaking").unwrap();
        assert_eq!((p.name.as_str(), p.speaking), ("Dana Smith", true));
        // Sobrenome primeiro, com vírgula: o nome é o que sobra.
        let p = parse_label("Smith, Dana, Presenter, Muted").unwrap();
        assert_eq!(p.name, "Smith, Dana");
        let p = parse_label("Orador ativo: João Ávila").unwrap();
        assert_eq!((p.name.as_str(), p.speaking), ("João Ávila", true));
        let p = parse_label("Active speaker, Dana Smith").unwrap();
        assert_eq!(p.name, "Dana Smith");
        let p = parse_label("Mariana Conceição está falando").unwrap();
        assert_eq!((p.name.as_str(), p.speaking), ("Mariana Conceição", true));
        let p = parse_label("Bruno Lima (Você), Organizador, Microfone desativado").unwrap();
        assert_eq!((p.name.as_str(), p.me), ("Bruno Lima", true));
        let p = parse_label("Bruno Lima (You)").unwrap();
        assert!(p.me && !p.status);
    }

    #[test]
    fn rotulos_que_nao_sao_de_pessoa() {
        for label in [
            "",
            "Sem mudo",
            "Organizador, Sem mudo",
            "Ana Souza, Organizador",
            "Ana Souza",
            "Falando",
            "Orador ativo:",
            "10:30, Sem mudo",
            "https://teams.microsoft.com, Muted",
            "Participantes (3)",
            "Chat da reunião",
        ] {
            assert_eq!(parse_label(label), None, "{label:?}");
        }
    }

    #[test]
    fn painel_pessoas_em_portugues() {
        let r = read_call(&fixture("pt-br-grupo-3.json"));
        assert_eq!(
            r.participants,
            ["Bruno Lima", "Ana Souza", "Carla Ávila Nogueira"]
        );
        assert_eq!(r.me.as_deref(), Some("Bruno Lima"));
        assert_eq!(r.speaking, ["Ana Souza"]);
    }

    #[test]
    fn painel_pessoas_em_ingles() {
        let r = read_call(&fixture("en-us-group-3.json"));
        assert_eq!(r.participants, ["Dana Smith", "Lee Park", "Sam Rivera"]);
        assert_eq!(r.me.as_deref(), Some("Dana Smith"));
        assert_eq!(r.speaking, ["Sam Rivera"]);
    }

    #[test]
    fn chamada_de_dois_sem_marca_de_quem_fala() {
        let r = read_call(&fixture("pt-br-1a1.json"));
        assert_eq!(r.participants, ["Bruno Lima", "José Antônio"]);
        assert_eq!(r.me.as_deref(), Some("Bruno Lima"));
        assert!(r.speaking.is_empty());
    }

    #[test]
    fn chat_e_legendas_nao_marcam_ninguem() {
        let r = read_call(&fixture("pt-br-chat-aberto.json"));
        // "Ana Souza está falando" escrito no chat e "Carla: …" nas legendas
        // não contam; o painel continua valendo.
        assert!(r.speaking.is_empty(), "{:?}", r.speaking);
        assert_eq!(r.participants.len(), 3);
    }

    #[test]
    fn galeria_nao_e_lista_de_presenca() {
        // Blocos de vídeo com estado, sem o painel Pessoas aberto.
        let w = UiWindow {
            title: "Reunião | Microsoft Teams".into(),
            tree: UiNode::with(
                "group",
                Some("Galeria"),
                vec![
                    UiNode::leaf("group", "Ana Souza, Sem mudo, Vídeo desligado"),
                    UiNode::leaf("group", "José Antônio, Mudo"),
                ],
            ),
        };
        let r = read_call(&[w]);
        assert!(r.participants.is_empty());
        assert!(r.speaking.is_empty());
    }

    #[test]
    fn homonimos_aparecem_duas_vezes_e_o_cabecalho_nao_vira_pessoa() {
        let roster = UiNode::with(
            "tree",
            Some("Participantes"),
            vec![UiNode::with(
                "tree item",
                Some("Nesta reunião (3)"),
                vec![
                    UiNode::leaf("tree item", "Ana Souza, Sem mudo"),
                    UiNode::leaf("tree item", "Ana Souza, Mudo"),
                    UiNode::with(
                        "tree item",
                        None,
                        vec![
                            UiNode::leaf("text", "Bruno Lima (Você)"),
                            UiNode::leaf("image", "Microfone desativado"),
                        ],
                    ),
                ],
            )],
        );
        let r = read_call(&[UiWindow {
            title: String::new(),
            tree: roster,
        }]);
        assert_eq!(r.participants, ["Ana Souza", "Ana Souza", "Bruno Lima"]);
        assert_eq!(r.me.as_deref(), Some("Bruno Lima"));
    }

    #[test]
    fn so_janela_com_sair_e_microfone_e_de_chamada() {
        let call = fixture("pt-br-grupo-3.json");
        assert!(is_call_window(&call[0].tree));
        let en = fixture("en-us-group-3.json");
        assert!(is_call_window(&en[0].tree));
        let main = UiNode::with(
            "window",
            Some("Chat | Microsoft Teams"),
            vec![
                UiNode::leaf("button", "Sair"),
                UiNode::leaf("button", "Novo chat"),
            ],
        );
        assert!(!is_call_window(&main));
    }

    #[test]
    fn resumo_do_log_so_tem_contagens() {
        let s = summarize(&fixture("pt-br-grupo-3.json"));
        assert_eq!(s.roster_rows, 3);
        assert_eq!(s.speaking, 1);
        assert!(s.me_known);
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("Ana"), "{json}");
    }

    #[test]
    fn anonimizar_troca_nomes_e_mantem_o_que_a_leitura_usa() {
        let windows = fixture("pt-br-chat-aberto.json");
        let before = read_call(&windows);
        let anon = anonymize(&windows);
        let json = serde_json::to_string(&anon).unwrap();
        for name in ["Ana", "Bruno", "Carla", "orçamento"] {
            assert!(!json.contains(name), "{name} vazou: {json}");
        }
        let after = read_call(&anon);
        assert_eq!(after.participants.len(), before.participants.len());
        assert_eq!(after.me.is_some(), before.me.is_some());
        assert!(after.participants.iter().all(|p| p.starts_with("Pessoa ")));
        assert!(is_call_window(&anon[0].tree));
        // A mesma pessoa, o mesmo apelido.
        let again = anonymize(&windows);
        assert_eq!(serde_json::to_string(&again).unwrap(), json);
    }
}
