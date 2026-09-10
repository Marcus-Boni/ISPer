//! Detecção de chamada ativa (Teams) pelas sessões de áudio do Windows.
//!
//! A pergunta "estou numa reunião do Teams?" tem uma resposta objetiva no
//! WASAPI: em chamada, o Teams mantém uma sessão de áudio **ativa** no
//! microfone (captura) e, quase sempre, nos alto-falantes (reprodução). Fora
//! de chamada, as sessões ficam inativas ou nem existem. Não precisa de bot,
//! de API do Teams nem de olhar títulos de janela.
//!
//! O Teams novo é um app WebView2: quem abre o microfone é um processo filho
//! (`msedgewebview2.exe`), então a sondagem considera a **árvore** de processos
//! cujo ancestral é um dos executáveis procurados.
//!
//! [`CallTracker`] aplica histerese sobre leituras periódicas: uma chamada só
//! "começa" depois de N leituras positivas (o teste de microfone do Teams e
//! sons de notificação não contam) e só "termina" depois de M negativas.

use std::collections::HashSet;

/// Executáveis do Teams: o novo (`ms-teams.exe`) e o clássico (`Teams.exe`).
pub const TEAMS_PROCESSES: [&str; 2] = ["ms-teams.exe", "teams.exe"];

/// O que uma sondagem viu nos endpoints de áudio para o processo procurado.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CallSignal {
    /// Sessão ativa num endpoint de CAPTURA — o microfone está aberto.
    pub capture: bool,
    /// Sessão ativa num endpoint de REPRODUÇÃO — algo está tocando.
    pub render: bool,
}

impl CallSignal {
    pub fn any(&self) -> bool {
        self.capture || self.render
    }
}

/// Transição detectada pelo [`CallTracker`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallEvent {
    Started,
    Ended,
}

/// Histerese sobre leituras periódicas: `start_polls` positivas seguidas para
/// começar (o dobro quando só há reprodução, sem microfone — um vídeo no chat
/// não é uma chamada) e `end_polls` negativas seguidas para terminar.
#[derive(Debug, Clone)]
pub struct CallTracker {
    start_polls: u32,
    end_polls: u32,
    on_streak: u32,
    off_streak: u32,
    in_call: bool,
}

impl CallTracker {
    pub fn new(start_polls: u32, end_polls: u32) -> Self {
        Self {
            start_polls: start_polls.max(1),
            end_polls: end_polls.max(1),
            on_streak: 0,
            off_streak: 0,
            in_call: false,
        }
    }

    pub fn in_call(&self) -> bool {
        self.in_call
    }

    /// Registra uma leitura; devolve a transição, se houve.
    pub fn update(&mut self, signal: CallSignal) -> Option<CallEvent> {
        if signal.any() {
            self.on_streak += 1;
            self.off_streak = 0;
            let needed = if signal.capture {
                self.start_polls
            } else {
                self.start_polls * 2
            };
            if !self.in_call && self.on_streak >= needed {
                self.in_call = true;
                return Some(CallEvent::Started);
            }
        } else {
            self.off_streak += 1;
            self.on_streak = 0;
            if self.in_call && self.off_streak >= self.end_polls {
                self.in_call = false;
                return Some(CallEvent::Ended);
            }
        }
        None
    }

    /// Esquece o histórico (ex.: o usuário desligou a detecção).
    pub fn reset(&mut self) {
        self.on_streak = 0;
        self.off_streak = 0;
        self.in_call = false;
    }
}

/// PIDs dos processos cujo nome — ou o de algum ancestral próximo — está em
/// `names`. Cobre o Teams novo: o áudio toca nos filhos WebView2.
pub fn process_tree_pids(names: &[&str]) -> HashSet<u32> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    let procs = sys.processes();
    let is_target = |pid: &sysinfo::Pid| {
        procs.get(pid).is_some_and(|p| {
            let name = p.name().to_string_lossy().to_lowercase();
            names.iter().any(|n| n.eq_ignore_ascii_case(&name))
        })
    };
    let mut out = HashSet::new();
    for pid in procs.keys() {
        let mut cursor = Some(*pid);
        // Até 8 níveis: app → browser do WebView2 → renderer/utility → …
        for _ in 0..8 {
            let Some(current) = cursor else { break };
            if is_target(&current) {
                out.insert(pid.as_u32());
                break;
            }
            cursor = procs.get(&current).and_then(|p| p.parent());
        }
    }
    out
}

/// Sonda os endpoints de áudio ativos e diz se algum processo de `names` (ou
/// filho) tem sessão ativa em captura e/ou reprodução. Barato (~10 ms);
/// inicializa COM na thread chamadora se preciso.
#[cfg(windows)]
pub fn probe(names: &[&str]) -> crate::Result<CallSignal> {
    use wasapi::{DeviceEnumerator, Direction, SessionState};

    wasapi::initialize_mta()
        .ok()
        .map_err(|e| crate::IsperError::Audio(format!("COM: {e}")))?;
    let pids = process_tree_pids(names);
    if pids.is_empty() {
        return Ok(CallSignal::default());
    }
    let enumerator = DeviceEnumerator::new()
        .map_err(|e| crate::IsperError::Audio(format!("enumerador de áudio: {e}")))?;

    // Falha num endpoint (desconectado no meio, sem sessões) não invalida os
    // outros: cada nível é um `let … else continue`.
    let any_active = |direction: Direction| -> bool {
        let Ok(devices) = enumerator.get_device_collection(&direction) else {
            return false;
        };
        let Ok(n) = devices.get_nbr_devices() else {
            return false;
        };
        for i in 0..n {
            let Ok(device) = devices.get_device_at_index(i) else {
                continue;
            };
            let Ok(manager) = device.get_iaudiosessionmanager() else {
                continue;
            };
            let Ok(sessions) = manager.get_audiosessionenumerator() else {
                continue;
            };
            let Ok(count) = sessions.get_count() else {
                continue;
            };
            for j in 0..count {
                let Ok(session) = sessions.get_session(j) else {
                    continue;
                };
                if !matches!(session.get_state(), Ok(SessionState::Active)) {
                    continue;
                }
                if session
                    .get_process_id()
                    .is_ok_and(|pid| pids.contains(&pid))
                {
                    return true;
                }
            }
        }
        false
    };

    Ok(CallSignal {
        capture: any_active(Direction::Capture),
        render: any_active(Direction::Render),
    })
}

/// Fora do Windows não há WASAPI: nunca há chamada.
#[cfg(not(windows))]
pub fn probe(_names: &[&str]) -> crate::Result<CallSignal> {
    Ok(CallSignal::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIC: CallSignal = CallSignal {
        capture: true,
        render: true,
    };
    const SPEAKERS_ONLY: CallSignal = CallSignal {
        capture: false,
        render: true,
    };
    const QUIET: CallSignal = CallSignal {
        capture: false,
        render: false,
    };

    #[test]
    fn comeca_so_depois_de_n_leituras_com_microfone() {
        let mut t = CallTracker::new(2, 3);
        assert_eq!(t.update(MIC), None);
        assert_eq!(t.update(MIC), Some(CallEvent::Started));
        assert!(t.in_call());
        // Já em chamada: leituras positivas não repetem o evento.
        assert_eq!(t.update(MIC), None);
    }

    #[test]
    fn uma_leitura_isolada_nao_conta() {
        let mut t = CallTracker::new(2, 3);
        assert_eq!(t.update(MIC), None);
        assert_eq!(t.update(QUIET), None);
        assert_eq!(t.update(MIC), None); // a sequência recomeçou
        assert!(!t.in_call());
    }

    #[test]
    fn so_reproducao_exige_o_dobro_de_leituras() {
        let mut t = CallTracker::new(2, 3);
        assert_eq!(t.update(SPEAKERS_ONLY), None);
        assert_eq!(t.update(SPEAKERS_ONLY), None);
        assert_eq!(t.update(SPEAKERS_ONLY), None);
        assert_eq!(t.update(SPEAKERS_ONLY), Some(CallEvent::Started));
    }

    #[test]
    fn termina_depois_de_m_leituras_negativas() {
        let mut t = CallTracker::new(1, 3);
        assert_eq!(t.update(MIC), Some(CallEvent::Started));
        assert_eq!(t.update(QUIET), None);
        assert_eq!(t.update(QUIET), None);
        // Uma leitura positiva no meio zera a contagem de silêncio.
        assert_eq!(t.update(MIC), None);
        assert_eq!(t.update(QUIET), None);
        assert_eq!(t.update(QUIET), None);
        assert_eq!(t.update(QUIET), Some(CallEvent::Ended));
        assert!(!t.in_call());
    }

    #[test]
    fn reset_esquece_tudo() {
        let mut t = CallTracker::new(1, 1);
        t.update(MIC);
        t.reset();
        assert!(!t.in_call());
        assert_eq!(t.update(QUIET), None);
    }

    #[test]
    fn arvore_de_processos_inclui_o_proprio_processo() {
        // O processo de teste aparece na árvore quando procurado pelo nome.
        let me = std::env::current_exe()
            .ok()
            .and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let pids = process_tree_pids(&[me.as_str()]);
        assert!(pids.contains(&std::process::id()));
        assert!(process_tree_pids(&["nao-existe-isper.exe"]).is_empty());
    }
}
