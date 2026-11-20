//! A árvore de acessibilidade de uma janela, do jeito que o leitor a copia
//! (UI Automation no Windows) e os testes a escrevem à mão.
//!
//! As chaves do JSON são curtas (`r`, `t`, `d`, `i`, `c`, `k`) para as
//! fixtures caberem numa tela; são as mesmas do Yogurt, de onde veio a ideia
//! de ler a chamada pela árvore. O que alguém digita nunca entra: o valor de
//! um campo de edição não é pedido ao UI Automation, só o nome acessível.

use serde::{Deserialize, Serialize};

/// No máximo tantos elementos de uma janela entram numa leitura.
pub const MAX_NODES: usize = 4000;
/// E no máximo até esta profundidade.
pub const MAX_DEPTH: usize = 40;
/// Texto mais longo que isto é cortado: nomes e rótulos são curtos.
pub const MAX_TEXT: usize = 300;
/// Janelas de chamada lidas por vez, no máximo.
pub const MAX_WINDOWS: usize = 4;

/// Um elemento da árvore.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiNode {
    /// Tipo de controle, com o nome do UI Automation em minúsculas
    /// (`button`, `list item`, `tree item`, `text`, `group`…; ver
    /// [`uia_role`]).
    #[serde(rename = "r")]
    pub role: String,
    /// Nome acessível (o `aria-label` de uma página do WebView2).
    #[serde(rename = "t", default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Texto de ajuda, quando difere do nome.
    #[serde(rename = "d", default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    /// `AutomationId` (o `id` do elemento numa página).
    #[serde(rename = "i", default, skip_serializing_if = "Option::is_none")]
    pub automation_id: Option<String>,
    /// Classes do DOM, quando o elemento vem de uma página (Chromium as
    /// expõe como `ClassName`).
    #[serde(rename = "c", default, skip_serializing_if = "Vec::is_empty")]
    pub classes: Vec<String>,
    /// Os filhos, em ordem.
    #[serde(rename = "k", default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<UiNode>,
}

/// Uma janela de nível superior: título e árvore.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiWindow {
    /// Título da janela.
    #[serde(default)]
    pub title: String,
    /// A árvore a partir da raiz da janela.
    pub tree: UiNode,
}

impl UiNode {
    /// Um elemento sem filhos, para os testes e as fixtures.
    pub fn leaf(role: &str, name: &str) -> Self {
        Self {
            role: role.into(),
            name: Some(name.into()),
            ..Self::default()
        }
    }

    /// Um elemento com filhos.
    pub fn with(role: &str, name: Option<&str>, children: Vec<UiNode>) -> Self {
        Self {
            role: role.into(),
            name: name.map(Into::into),
            children,
            ..Self::default()
        }
    }

    /// Os textos do elemento: nome e ajuda, os que não estão vazios.
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        [self.name.as_deref(), self.help.as_deref()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .filter(|t| !t.is_empty())
    }

    /// Visita cada elemento com a lista dos ancestrais (raiz primeiro).
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a UiNode, &[&'a UiNode])) {
        let mut up: Vec<&'a UiNode> = Vec::new();
        self.walk_inner(&mut up, f);
    }

    fn walk_inner<'a>(
        &'a self,
        up: &mut Vec<&'a UiNode>,
        f: &mut impl FnMut(&'a UiNode, &[&'a UiNode]),
    ) {
        f(self, up);
        up.push(self);
        for k in &self.children {
            k.walk_inner(up, f);
        }
        up.pop();
    }

    /// Quantos elementos a árvore tem, a raiz incluída.
    pub fn count(&self) -> usize {
        1 + self.children.iter().map(UiNode::count).sum::<usize>()
    }

    /// A profundidade da árvore (só a raiz = 1).
    pub fn depth(&self) -> usize {
        1 + self.children.iter().map(UiNode::depth).max().unwrap_or(0)
    }
}

/// Corta um texto em [`MAX_TEXT`] caracteres, sem partir um caractere, e tira
/// os espaços das pontas (o espaço não separável vira espaço comum).
pub fn clip(s: &str) -> String {
    let s = s.replace('\u{a0}', " ");
    let s = s.trim();
    match s.char_indices().nth(MAX_TEXT) {
        Some((i, _)) => s[..i].to_string(),
        None => s.to_string(),
    }
}

/// O nome do tipo de controle do UI Automation (`UIA_*ControlTypeId`), em
/// minúsculas e com espaço — o vocabulário que [`super::teams`] reconhece.
pub fn uia_role(control_type: i32) -> &'static str {
    match control_type {
        50000 => "button",
        50001 => "calendar",
        50002 => "check box",
        50003 => "combo box",
        50004 => "edit",
        50005 => "hyperlink",
        50006 => "image",
        50007 => "list item",
        50008 => "list",
        50009 => "menu",
        50010 => "menu bar",
        50011 => "menu item",
        50012 => "progress bar",
        50013 => "radio button",
        50014 => "scroll bar",
        50015 => "slider",
        50016 => "spinner",
        50017 => "status bar",
        50018 => "tab",
        50019 => "tab item",
        50020 => "text",
        50021 => "tool bar",
        50022 => "tool tip",
        50023 => "tree",
        50024 => "tree item",
        50025 => "custom",
        50026 => "group",
        50027 => "thumb",
        50028 => "data grid",
        50029 => "data item",
        50030 => "document",
        50031 => "split button",
        50032 => "window",
        50033 => "pane",
        50034 => "header",
        50035 => "header item",
        50036 => "table",
        50037 => "title bar",
        50038 => "separator",
        50039 => "semantic zoom",
        50040 => "app bar",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_curto_vai_e_volta() {
        let tree = UiNode::with(
            "list",
            Some("Participantes"),
            vec![UiNode::leaf(
                "list item",
                "Ana Souza, Organizador, Sem mudo",
            )],
        );
        let json = serde_json::to_string(&tree).unwrap();
        assert_eq!(
            json,
            r#"{"r":"list","t":"Participantes","k":[{"r":"list item","t":"Ana Souza, Organizador, Sem mudo"}]}"#
        );
        let back: UiNode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tree);
        assert_eq!(back.count(), 2);
        assert_eq!(back.depth(), 2);
    }

    #[test]
    fn walk_entrega_os_ancestrais_em_ordem() {
        let tree = UiNode::with(
            "window",
            Some("w"),
            vec![UiNode::with(
                "list",
                Some("l"),
                vec![UiNode::leaf("list item", "x")],
            )],
        );
        let mut seen = Vec::new();
        tree.walk(&mut |n, up| {
            let path: Vec<&str> = up.iter().map(|a| a.role.as_str()).collect();
            seen.push(format!("{}<{}", n.role, path.join("/")));
        });
        assert_eq!(seen, ["window<", "list<window", "list item<window/list"]);
    }

    #[test]
    fn clip_corta_sem_partir_caractere_e_limpa_espacos() {
        let long = "é".repeat(MAX_TEXT + 10);
        assert_eq!(clip(&long).chars().count(), MAX_TEXT);
        assert_eq!(clip("\u{a0} Ana  "), "Ana");
    }

    #[test]
    fn papeis_do_uia_conhecidos_e_desconhecidos() {
        assert_eq!(uia_role(50000), "button");
        assert_eq!(uia_role(50024), "tree item");
        assert_eq!(uia_role(1), "unknown");
    }
}
