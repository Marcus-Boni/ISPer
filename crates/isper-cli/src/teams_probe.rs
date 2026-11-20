//! `isper-cli teams-probe`: a sonda da rodada de validação dos nomes do
//! Teams (plano, seção 1). Com uma chamada aberta, lê a janela dela como o
//! leitor do app lê, e mostra a cada leitura o que daria para usar: quantas
//! linhas o painel Pessoas tem, quantas pessoas o Teams marca falando, se
//! "(Você)" foi reconhecido — e, com `--names`, os nomes (só no terminal).
//!
//! Com `--fixture`, grava a última árvore lida **anonimizada** para virar
//! fixture de teste (`fixtures/teams`): nomes viram "Pessoa A"…, e o resto
//! do texto que não é do Teams vira só o tamanho dele.
//!
//! Para medir o atraso da tela, alterne quem fala em horários conhecidos
//! (anote o relógio) e compare com o horário que a sonda imprime quando a
//! marca muda.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Context;

#[cfg(windows)]
pub(crate) fn run(
    seconds: u64,
    every_ms: u64,
    names: bool,
    fixture: Option<&Path>,
) -> anyhow::Result<()> {
    use isper_core::loopback::find_root_pid;
    use isper_core::speaker_names::{teams, uia::UiaReader};

    let root = find_root_pid(&["ms-teams.exe".into(), "teams.exe".into()])
        .context("o Teams não está rodando (ms-teams.exe / teams.exe)")?;
    let reader = UiaReader::new().map_err(anyhow::Error::msg)?;
    println!(
        "Teams: PID raiz {root}. Lendo por {seconds} s, a cada {every_ms} ms (Ctrl+C para parar)."
    );
    let started = Instant::now();
    let mut last_said = String::new();
    let mut last_windows = Vec::new();
    while started.elapsed() < Duration::from_secs(seconds) {
        let t0 = Instant::now();
        let snap = reader.read(root);
        let took = t0.elapsed();
        let r = isper_core::speaker_names::read_call(&snap.windows);
        let s = teams::summarize(&snap.windows);
        let mut said = format!(
            "{:?} · janelas {} · elementos {} · painel {} · falando {} · você {}",
            snap.state,
            s.windows,
            s.nodes,
            s.roster_rows,
            s.speaking,
            if s.me_known { "sim" } else { "não" }
        );
        if names {
            said.push_str(&format!(
                " · falando: [{}] · painel: [{}] · você: {}",
                r.speaking.join(", "),
                r.participants.join(", "),
                r.me.as_deref().unwrap_or("?")
            ));
        }
        // Só as mudanças, com o horário: é o que se compara com as anotações.
        if said != last_said {
            println!(
                "{} (+{:.1} s, leitura {} ms) {said}",
                chrono::Local::now().format("%H:%M:%S%.3f"),
                started.elapsed().as_secs_f32(),
                took.as_millis()
            );
            last_said = said;
        }
        if !snap.windows.is_empty() {
            last_windows = snap.windows;
        }
        let took = took.as_millis() as u64;
        std::thread::sleep(Duration::from_millis(
            every_ms.max(took * 2).saturating_sub(took),
        ));
    }
    if let Some(path) = fixture {
        anyhow::ensure!(
            !last_windows.is_empty(),
            "nenhuma janela de chamada foi lida: nada a gravar"
        );
        let anon = teams::anonymize(&last_windows);
        std::fs::write(path, serde_json::to_string_pretty(&anon)?)
            .with_context(|| format!("gravando {}", path.display()))?;
        println!(
            "Árvore anonimizada em {} — confira antes de fazer commit.",
            path.display()
        );
    }
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn run(_: u64, _: u64, _: bool, _: Option<&Path>) -> anyhow::Result<()> {
    anyhow::bail!("a leitura da chamada do Teams só existe no Windows")
}
