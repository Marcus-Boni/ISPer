//! Leitura das janelas de chamada do Teams pelo UI Automation do Windows.
//!
//! Só janelas de nível superior de processos da ÁRVORE do Teams que está
//! sendo gravado (o PID raiz que o loopback abriu e os filhos dele — o Teams
//! novo desenha a chamada num WebView2 filho). Cada janela vem inteira numa
//! chamada só ao Teams (cache do UI Automation com as cinco propriedades
//! necessárias), com tempo limite de conexão e de transação: um Teams que
//! parou de responder perde a leitura, não trava quem lê.
//!
//! Nenhum clique, nenhum foco, nenhum valor de campo de edição: só o que o
//! leitor de tela do Windows também leria.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::UI::Accessibility::{
    AutomationElementMode_None, CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomation2,
    IUIAutomationCacheRequest, IUIAutomationElement, TreeScope_Subtree, UIA_AutomationIdPropertyId,
    UIA_ClassNamePropertyId, UIA_ControlTypePropertyId, UIA_HelpTextPropertyId, UIA_NamePropertyId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
};
use windows::core::{BOOL, Interface};

use super::attribute::ObservationState;
use super::teams::{is_call_window, read_call};
use super::tree::{MAX_DEPTH, MAX_NODES, MAX_WINDOWS, UiNode, UiWindow, clip, uia_role};

/// Um app que parou de responder perde a leitura depois disto.
const TIMEOUT_MS: u32 = 1500;
/// Uma janela que não é de chamada (a principal, com o chat) só é olhada de
/// novo depois disto — ler a árvore inteira dela a cada meio segundo seria
/// trabalho jogado fora.
const RECHECK_NON_CALL: Duration = Duration::from_secs(5);

/// O resultado de uma leitura.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Se vale (ver [`ObservationState`]).
    pub state: ObservationState,
    /// A janela de chamada lida, quando `state` é `Ok`.
    pub windows: Vec<UiWindow>,
}

/// O leitor: o UI Automation aberto e o pedido de cache pronto. Vive numa
/// thread só (COM em modo MTA, iniciado em [`UiaReader::new`]).
pub struct UiaReader {
    uia: IUIAutomation,
    req: IUIAutomationCacheRequest,
    non_call: RefCell<HashMap<isize, Instant>>,
}

impl UiaReader {
    /// Abre o UI Automation nesta thread.
    pub fn new() -> Result<Self, String> {
        // SAFETY: chamadas COM documentadas; a thread entra no MTA (o
        // UI Automation é seguro nele) e os objetos ficam nesta struct, que
        // não sai da thread (`IUIAutomation` não é `Send`).
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let uia: IUIAutomation =
                match CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) {
                    Ok(u) => u,
                    Err(_) => CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                        .map_err(|e| format!("UI Automation: {e}"))?,
                };
            if let Ok(u2) = uia.cast::<IUIAutomation2>() {
                let _ = u2.SetConnectionTimeout(TIMEOUT_MS);
                let _ = u2.SetTransactionTimeout(TIMEOUT_MS);
            }
            let req = uia
                .CreateCacheRequest()
                .map_err(|e| format!("cache do UI Automation: {e}"))?;
            for p in [
                UIA_ControlTypePropertyId,
                UIA_NamePropertyId,
                UIA_HelpTextPropertyId,
                UIA_AutomationIdPropertyId,
                UIA_ClassNamePropertyId,
            ] {
                req.AddProperty(p).map_err(|e| e.to_string())?;
            }
            req.SetTreeScope(TreeScope_Subtree)
                .map_err(|e| e.to_string())?;
            // A visão "crua" mantém os contêineres simples de uma página,
            // onde o painel Pessoas guarda as linhas.
            let raw = uia.RawViewCondition().map_err(|e| e.to_string())?;
            req.SetTreeFilter(&raw).map_err(|e| e.to_string())?;
            req.SetAutomationElementMode(AutomationElementMode_None)
                .map_err(|e| e.to_string())?;
            Ok(Self {
                uia,
                req,
                non_call: RefCell::new(HashMap::new()),
            })
        }
    }

    /// Lê as janelas de chamada da árvore de processos de `root_pid`.
    pub fn read(&self, root_pid: u32) -> Snapshot {
        let pids = process_tree(root_pid);
        if !pids.contains(&root_pid) {
            return Snapshot {
                state: ObservationState::NotRunning,
                windows: Vec::new(),
            };
        }
        let mut calls: Vec<UiWindow> = Vec::new();
        let now = Instant::now();
        self.non_call
            .borrow_mut()
            .retain(|_, at| now.duration_since(*at) < RECHECK_NON_CALL);
        for (hwnd, title) in top_windows(&pids) {
            if calls.len() == MAX_WINDOWS {
                break;
            }
            if self.non_call.borrow().contains_key(&(hwnd.0 as isize)) {
                continue;
            }
            let Some(tree) = self.tree(hwnd) else {
                continue;
            };
            if is_call_window(&tree) {
                calls.push(UiWindow {
                    title: clip(&title),
                    tree,
                });
            } else {
                self.non_call.borrow_mut().insert(hwnd.0 as isize, now);
            }
        }
        decide(calls)
    }

    fn tree(&self, hwnd: HWND) -> Option<UiNode> {
        // SAFETY: `hwnd` veio do EnumWindows agora há pouco; se a janela
        // fechou no meio, a chamada falha e a leitura dela é pulada.
        let el = unsafe { self.uia.ElementFromHandleBuildCache(hwnd, &self.req) }.ok()?;
        let mut budget = MAX_NODES;
        Some(node(&el, 0, &mut budget))
    }
}

/// Mais de uma janela de chamada: só vale se uma única tiver informação (a
/// janela compacta da chamada minimizada costuma ter só os botões). Duas com
/// gente dentro podem ser duas chamadas — uma em espera —, e aí não se sabe
/// qual é a gravada.
pub fn decide(calls: Vec<UiWindow>) -> Snapshot {
    if calls.is_empty() {
        return Snapshot {
            state: ObservationState::NoCallWindow,
            windows: calls,
        };
    }
    let informative: Vec<usize> = calls
        .iter()
        .enumerate()
        .filter(|(_, w)| {
            let r = read_call(std::slice::from_ref(w));
            !r.participants.is_empty() || !r.speaking.is_empty()
        })
        .map(|(i, _)| i)
        .collect();
    match informative.as_slice() {
        [] | [_] if calls.len() == 1 => Snapshot {
            state: ObservationState::Ok,
            windows: calls,
        },
        [] => Snapshot {
            state: ObservationState::Ok,
            windows: Vec::new(),
        },
        [i] => Snapshot {
            state: ObservationState::Ok,
            windows: vec![calls[*i].clone()],
        },
        _ => Snapshot {
            state: ObservationState::Ambiguous,
            windows: Vec::new(),
        },
    }
}

/// Um elemento e o que há debaixo dele, até o orçamento de nós e a
/// profundidade máxima.
fn node(el: &IUIAutomationElement, depth: usize, budget: &mut usize) -> UiNode {
    *budget = budget.saturating_sub(1);
    let text = |b: windows::core::Result<windows::core::BSTR>| {
        b.ok()
            .map(|b| clip(&b.to_string()))
            .filter(|s| !s.is_empty())
    };
    // SAFETY: propriedades em cache de um elemento obtido com o pedido de
    // cache acima; ler uma que falte só devolve erro.
    unsafe {
        let role = el
            .CachedControlType()
            .map(|t| uia_role(t.0))
            .unwrap_or("unknown");
        let name = text(el.CachedName());
        let help = text(el.CachedHelpText()).filter(|h| Some(h) != name.as_ref());
        let automation_id = text(el.CachedAutomationId());
        let classes: Vec<String> = text(el.CachedClassName())
            .map(|c| c.split_whitespace().take(20).map(str::to_string).collect())
            .unwrap_or_default();
        let mut children = Vec::new();
        if depth < MAX_DEPTH
            && *budget > 0
            && let Ok(kids) = el.GetCachedChildren()
        {
            for i in 0..kids.Length().unwrap_or(0) {
                if *budget == 0 {
                    break;
                }
                if let Ok(k) = kids.GetElement(i) {
                    children.push(node(&k, depth + 1, budget));
                }
            }
        }
        UiNode {
            role: role.to_string(),
            name,
            help,
            automation_id,
            classes,
            children,
        }
    }
}

/// `root` e todos os descendentes dele (o Teams novo: `ms-teams.exe` →
/// `msedgewebview2.exe`…).
pub fn process_tree(root: u32) -> HashSet<u32> {
    let mut parent_of: Vec<(u32, u32)> = Vec::new();
    // SAFETY: instantâneo de processos do Toolhelp; o handle é fechado no
    // fim, e `entry.dwSize` é preenchido antes do primeiro uso.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return HashSet::new();
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                parent_of.push((entry.th32ProcessID, entry.th32ParentProcessID));
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    descendants(root, &parent_of)
}

/// Fecho transitivo dos filhos de `root` numa lista (pid, pai). Separado
/// para ser testável sem o Windows. Um PID reaproveitado que aponte para um
/// pai que já morreu não entra: o pai precisa estar no conjunto.
pub fn descendants(root: u32, parent_of: &[(u32, u32)]) -> HashSet<u32> {
    let mut set = HashSet::new();
    if !parent_of.iter().any(|(pid, _)| *pid == root) {
        return set;
    }
    set.insert(root);
    loop {
        let before = set.len();
        for (pid, parent) in parent_of {
            if *pid != *parent && set.contains(parent) {
                set.insert(*pid);
            }
        }
        if set.len() == before {
            return set;
        }
    }
}

/// Janelas visíveis de nível superior dos processos dados, com o título.
fn top_windows(pids: &HashSet<u32>) -> Vec<(HWND, String)> {
    struct Ctx<'a> {
        pids: &'a HashSet<u32>,
        out: Vec<(HWND, String)>,
    }
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: `lparam` é o `&mut Ctx` passado logo abaixo, vivo durante
        // todo o EnumWindows.
        let ctx = unsafe { &mut *(lparam.0 as *mut Ctx<'_>) };
        let mut pid = 0u32;
        // SAFETY: `hwnd` vem do próprio EnumWindows.
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if ctx.pids.contains(&pid) && IsWindowVisible(hwnd).as_bool() {
                let mut buf = [0u16; 512];
                let n = GetWindowTextW(hwnd, &mut buf).max(0) as usize;
                ctx.out
                    .push((hwnd, String::from_utf16_lossy(&buf[..n.min(buf.len())])));
            }
        }
        BOOL(1)
    }
    let mut ctx = Ctx {
        pids,
        out: Vec::new(),
    };
    // SAFETY: a callback só usa `ctx` enquanto o EnumWindows roda.
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut ctx as *mut Ctx<'_> as isize));
    }
    ctx.out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descendentes_seguem_a_arvore_e_param_nos_estranhos() {
        // 10 = ms-teams; 11 e 12 = webviews dele; 13 = filho do 12; 20 =
        // outro programa; 30 aponta para um pai (99) que não existe mais.
        let tree = [
            (10, 1),
            (11, 10),
            (12, 10),
            (13, 12),
            (20, 1),
            (30, 99),
            (1, 0),
        ];
        let d = descendants(10, &tree);
        assert_eq!(d, HashSet::from([10, 11, 12, 13]));
        assert!(descendants(99, &tree).is_empty(), "raiz que não existe");
        // Um processo que é pai de si mesmo (PID 0 do sistema) não trava.
        assert_eq!(descendants(0, &[(0, 0)]), HashSet::from([0]));
    }

    fn call_window(rows: &[&str]) -> UiWindow {
        let mut kids = vec![
            UiNode::leaf("button", "Sair"),
            UiNode::leaf("button", "Ativar mudo"),
        ];
        if !rows.is_empty() {
            kids.push(UiNode::with(
                "tree",
                Some("Participantes"),
                rows.iter().map(|r| UiNode::leaf("tree item", r)).collect(),
            ));
        }
        UiWindow {
            title: "Reunião".into(),
            tree: UiNode::with("window", None, kids),
        }
    }

    #[test]
    fn varias_janelas_de_chamada() {
        assert_eq!(decide(Vec::new()).state, ObservationState::NoCallWindow);
        let one = decide(vec![call_window(&["Ana, Sem mudo"])]);
        assert_eq!((one.state, one.windows.len()), (ObservationState::Ok, 1));
        // A janela compacta (só botões) e a chamada: vale a chamada.
        let mini = decide(vec![call_window(&[]), call_window(&["Ana, Sem mudo"])]);
        assert_eq!(mini.state, ObservationState::Ok);
        assert_eq!(read_call(&mini.windows).participants, ["Ana"]);
        // Duas chamadas com gente: qual é a gravada?
        let two = decide(vec![
            call_window(&["Ana, Sem mudo"]),
            call_window(&["José, Mudo"]),
        ]);
        assert_eq!(two.state, ObservationState::Ambiguous);
        assert!(two.windows.is_empty());
    }
}
