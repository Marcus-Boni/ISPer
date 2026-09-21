//! Dados com responsabilidade (fase 7.4): retenção de reuniões e ditados
//! (LGPD), backup do banco, pacote de diagnóstico e métricas locais.
//!
//! Nada aqui sai da máquina: não há telemetria remota — nem opt-in. As
//! métricas moram no banco do usuário e aparecem em Configurações →
//! Diagnóstico; o pacote de diagnóstico é um arquivo que só viaja se o
//! usuário o mandar.

use std::os::windows::process::CommandExt;

use crate::prelude::*;
use isper_core::store::Purged;

/// Instante "agora" no relógio das datas do app (local, sem fuso) — o mesmo
/// de [`isper_core::store::parse_local_stamp`], que só serve para comparar.
pub(crate) fn local_now_ts() -> i64 {
    chrono::Local::now().naive_local().and_utc().timestamp()
}

/// Janela das métricas mostradas no Diagnóstico e no pacote exportado.
pub(crate) const METRICS_WINDOW_DAYS: i64 = 30;

/// Tipos de evento registrados nas métricas locais.
pub(crate) const EVENT_DICTATION: &str = "dictation";
pub(crate) const EVENT_MEETING_BLOCK: &str = "meeting_block";

/// Primeira varredura de retenção depois de o app assentar; depois uma por dia.
const FIRST_SWEEP_DELAY: Duration = Duration::from_secs(30);
const SWEEP_EVERY: Duration = Duration::from_secs(24 * 3600);

/// Logs incluídos no pacote de diagnóstico (os mais recentes).
const DIAG_LOG_FILES: usize = 3;

/// Backups automáticos da retenção que ficam guardados (os mais recentes).
const RETENTION_BACKUPS_KEEP: usize = 3;

/// Prefixo dos backups automáticos que a retenção grava antes de apagar.
const RETENTION_BACKUP_PREFIX: &str = "isper-antes-da-retencao-";

/// Registra um evento de métrica (`kind`, sucesso, inferência, áudio). Falha
/// só vai ao log em `debug`: métrica nunca atrasa nem derruba o fluxo.
pub(crate) fn record_event(kind: &str, ok: bool, secs: Option<f32>, audio_secs: Option<f32>) {
    let result = open_store().and_then(|store| {
        store
            .record_event(local_now_ts(), kind, ok, secs, audio_secs)
            .map_err(anyhow::Error::from)
    });
    if let Err(e) = result {
        tracing::debug!("métrica não registrada ({kind}): {e}");
    }
}

// ------------------------------------------------------------- retenção

/// Aplica a política de retenção uma vez: apaga do banco o que passou de
/// `retention_days` e, em disco, o Markdown de cada reunião apagada (e os
/// SRT/DOCX exportados ao lado dele). `Ok(None)` = retenção desligada — o
/// padrão: nada some sem o usuário ter escolhido um prazo. Antes de apagar
/// qualquer coisa, grava um backup automático do banco (os três últimos
/// ficam em `Documentos\ISPer\Backups`): o que sair continua recuperável.
pub(crate) fn retention_sweep(app: &AppHandle) -> anyhow::Result<Option<Purged>> {
    let days = app
        .state::<AppState>()
        .config
        .lock_or_recover()
        .retention_days;
    if days == 0 {
        return Ok(None);
    }
    let cutoff = local_now_ts() - i64::from(days) * 86_400;
    let store = open_store()?;
    let (old_meetings, old_dictations) = store.count_older_than(cutoff)?;
    if old_meetings == 0 && old_dictations == 0 {
        return Ok(Some(Purged::default()));
    }
    // Rede de segurança: sem backup, não apaga.
    let backup = safety_backup(&store)?;
    tracing::info!(
        path = %backup.display(),
        meetings = old_meetings,
        dictations = old_dictations,
        "backup automático antes da retenção"
    );
    let purged = store.purge_older_than(cutoff)?;
    let mut files = 0usize;
    for path in purged.meetings.iter().filter_map(|m| m.md_path.as_deref()) {
        files += remove_meeting_files(Path::new(path));
    }
    if !purged.meetings.is_empty() || purged.dictations > 0 {
        tracing::info!(
            days,
            meetings = purged.meetings.len(),
            files,
            dictations = purged.dictations,
            "retenção aplicada"
        );
    }
    Ok(Some(purged))
}

/// Backup automático da retenção (`isper-antes-da-retencao-<data>.db`), com
/// os mais antigos além de [`RETENTION_BACKUPS_KEEP`] apagados.
fn safety_backup(store: &isper_core::store::MeetingStore) -> anyhow::Result<PathBuf> {
    let dir = docs_dir()?.join("Backups");
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let dest = dir.join(format!("{RETENTION_BACKUP_PREFIX}{stamp}.db"));
    store.backup_to(&dest)?;
    prune_backups(&dir, RETENTION_BACKUP_PREFIX, RETENTION_BACKUPS_KEEP);
    Ok(dest)
}

/// Deixa só os `keep` arquivos mais recentes (pelo nome, que carrega a data)
/// entre os que começam com `prefix`. Só toca nos backups automáticos — os
/// que o usuário fez à mão têm outro nome e ficam.
fn prune_backups(dir: &Path, prefix: &str, keep: usize) {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|f| f.to_string_lossy().starts_with(prefix))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let excess = files.len().saturating_sub(keep);
    for path in files.into_iter().take(excess) {
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!(
                "não consegui apagar o backup antigo {}: {e}",
                path.display()
            );
        }
    }
}

/// Apaga o Markdown da reunião e as exportações com o mesmo nome (`.srt`,
/// `.docx`), se existirem. Devolve quantos arquivos saíram.
fn remove_meeting_files(md: &Path) -> usize {
    let mut removed = 0;
    let siblings = [
        md.to_path_buf(),
        md.with_extension("srt"),
        md.with_extension("docx"),
    ];
    for path in &siblings {
        match std::fs::remove_file(path) {
            Ok(()) => removed += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => tracing::warn!("retenção: não consegui apagar {}: {e}", path.display()),
        }
    }
    removed
}

/// Varredura em segundo plano: a primeira depois de o app assentar, depois
/// uma por dia — lendo a política a cada volta (mudou nas Configurações,
/// vale na próxima).
pub(crate) fn schedule_retention(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("isper-retention".into())
        .spawn(move || {
            let mut delay = FIRST_SWEEP_DELAY;
            loop {
                std::thread::sleep(delay);
                delay = SWEEP_EVERY;
                if let Err(e) = retention_sweep(&app) {
                    tracing::warn!("retenção não concluída: {e}");
                }
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("não consegui iniciar a varredura de retenção: {e}");
    }
}

/// Uma varredura agora, fora da thread de quem chamou (a tela de
/// Configurações não espera o banco).
pub(crate) fn sweep_in_background(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = retention_sweep(&app) {
            tracing::warn!("retenção não concluída: {e}");
        }
    });
}

// --------------------------------------------------------------- backup

/// `Documentos\ISPer` — a pasta que o usuário vê (as reuniões ficam em
/// `Reunioes`, os backups em `Backups`).
fn docs_dir() -> anyhow::Result<PathBuf> {
    let dir = crate::paths::home_dir()
        .ok_or_else(|| anyhow::anyhow!("pasta do usuário indisponível"))?
        .join("Documents")
        .join("ISPer");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Cópia íntegra e compactada do banco em `Documentos\ISPer\Backups\isper-<data>.db`,
/// com o app aberto (`VACUUM INTO`). Devolve o caminho e o mostra no Explorer.
/// Restaurar é manual: fechar o ISPer e copiar o arquivo por cima de
/// `%APPDATA%\ISPer\isper.db`.
#[tauri::command]
pub(crate) fn backup_database() -> Result<String, String> {
    let dir = docs_dir().map_err(|e| e.to_string())?.join("Backups");
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let dest = dir.join(format!("isper-{stamp}.db"));
    let store = open_store().map_err(|e| e.to_string())?;
    store.backup_to(&dest).map_err(|e| e.to_string())?;
    tracing::info!(path = %dest.display(), "backup do banco gravado");
    reveal(&dest);
    Ok(dest.display().to_string())
}

/// Reconstrói a Biblioteca a partir dos `.md` em `Documentos\ISPer\Reunioes`.
///
/// O Markdown é gravado ANTES do banco e sobrevive a qualquer acidente com o
/// índice; este comando lê a pasta de volta e reinsere o que faltar. É seguro
/// repetir: uma reunião que já está no banco é pulada.
///
/// Devolve `(reimportadas, já no banco, com problema)`.
#[tauri::command]
pub(crate) fn reimport_meetings() -> Result<(usize, usize, usize), String> {
    let dir = meetings_dir().map_err(|e| e.to_string())?;
    let store = open_store().map_err(|e| e.to_string())?;
    let mut arquivos: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("não consegui ler {}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .collect();
    arquivos.sort();

    let (mut novas, mut existentes, mut falhas) = (0usize, 0usize, 0usize);
    for path in &arquivos {
        let caminho = path.to_string_lossy().to_string();
        match isper_core::import::parse_file(path) {
            Ok(Ok(m)) => match store.import_meeting(&m, &caminho) {
                Ok(Some(id)) => {
                    tracing::info!(id, arquivo = %caminho, "reunião reimportada do Markdown");
                    novas += 1;
                }
                Ok(None) => existentes += 1,
                Err(e) => {
                    tracing::warn!(arquivo = %caminho, "não consegui reimportar: {e}");
                    falhas += 1;
                }
            },
            Ok(Err(e)) => {
                tracing::warn!("Markdown não reconhecido: {e}");
                falhas += 1;
            }
            Err(e) => {
                tracing::warn!(arquivo = %caminho, "não consegui ler: {e}");
                falhas += 1;
            }
        }
    }
    tracing::info!(
        arquivos = arquivos.len(),
        novas,
        existentes,
        falhas,
        "reimportação da pasta concluída"
    );
    Ok((novas, existentes, falhas))
}

/// Abre o Explorer com `path` selecionado.
fn reveal(path: &Path) {
    let _ = std::process::Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn();
}

// ---------------------------------------------------------- diagnóstico

/// Pacote de diagnóstico para pedir ajuda, em
/// `Documentos\ISPer\diagnostico-ISPer-<data>.zip`: o diagnóstico da tela
/// (JSON), versões, `config.toml` e `llm.toml` (nenhum dos dois guarda
/// segredo — as chaves ficam no Credential Manager), as métricas dos últimos
/// 30 dias (dentro do JSON) e os últimos três logs, com as linhas que trazem
/// texto ditado substituídas por um marcador. Abre o Explorer no arquivo.
#[tauri::command]
pub(crate) fn export_diagnostics(app: AppHandle) -> Result<String, String> {
    let diag = diagnostics(app);
    let mut entries: Vec<(String, Vec<u8>)> = vec![
        (
            "diagnostico.json".into(),
            serde_json::to_vec_pretty(&diag).map_err(|e| e.to_string())?,
        ),
        ("versoes.txt".into(), versions_text().into_bytes()),
    ];
    if let Some(p) = config::path() {
        push_file(&mut entries, "config.toml", &p);
    }
    if let Some(p) = crate::paths::roaming_dir().map(|d| d.join("llm.toml")) {
        push_file(&mut entries, "llm.toml", &p);
    }
    if let Some(dir) = logs_dir() {
        for p in recent_logs(&dir, DIAG_LOG_FILES) {
            if let Ok(text) = std::fs::read_to_string(&p) {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "isper.log".into());
                entries.push((format!("logs/{name}"), redact_log(&text).into_bytes()));
            }
        }
    }
    let refs: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(name, data)| (name.as_str(), data.as_slice()))
        .collect();
    let zip = isper_core::export::zip_store(&refs);
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let dest = docs_dir()
        .map_err(|e| e.to_string())?
        .join(format!("diagnostico-ISPer-{stamp}.zip"));
    std::fs::write(&dest, zip).map_err(|e| e.to_string())?;
    tracing::info!(path = %dest.display(), entries = refs.len(), "diagnóstico exportado");
    reveal(&dest);
    Ok(dest.display().to_string())
}

fn push_file(entries: &mut Vec<(String, Vec<u8>)>, name: &str, path: &Path) {
    if let Ok(data) = std::fs::read(path) {
        entries.push((name.to_string(), data));
    }
}

/// Versões que importam para reproduzir um problema.
fn versions_text() -> String {
    let variant = if cfg!(feature = "cuda") {
        "GPU (CUDA)"
    } else {
        "CPU"
    };
    let os = sysinfo::System::long_os_version().unwrap_or_else(|| "Windows".into());
    let webview = tauri::webview_version().unwrap_or_else(|_| "desconhecida".into());
    format!(
        "ISPer {}\nvariante: {variant}\nTauri: {}\nWebView2: {webview}\nsistema: {os}\narquitetura: {}\n",
        env!("CARGO_PKG_VERSION"),
        tauri::VERSION,
        std::env::consts::ARCH,
    )
}

/// Os `n` logs mais recentes da pasta (`isper.log.<data>`; o nome ordena).
fn recent_logs(dir: &Path, n: usize) -> Vec<PathBuf> {
    let mut logs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|f| f.to_string_lossy().starts_with("isper.log"))
                })
                .collect()
        })
        .unwrap_or_default();
    logs.sort();
    logs.reverse();
    logs.truncate(n);
    logs
}

/// Marcador que substitui, no pacote exportado, as linhas de log com texto ditado.
const REDACTED_LINE: &str = "[linha com texto ditado removida do diagnóstico]";

/// Tira do log o que é conteúdo do usuário: a linha do ditado transcrito
/// (`transcrito: …`) carrega o texto falado. O resto — tempos, erros,
/// dispositivos — é o que o diagnóstico precisa.
fn redact_log(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        if line.contains("transcrito:") {
            out.push_str(REDACTED_LINE);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_troca_so_as_linhas_com_texto_ditado() {
        let log = "2026-09-13 INFO ISPer 0.15.0 iniciando\n\
                   2026-09-13 INFO audio_secs=3.1 infer_secs=0.4 transcrito: segredo de reunião\n\
                   {\"level\":\"INFO\",\"message\":\"transcrito: outro segredo\",\"infer_secs\":0.5}\n\
                   2026-09-13 INFO bloco transcrito speaker=Eu infer_secs=0.9\n\
                   2026-09-13 WARN ditado falhou: não entendi — tente de novo";
        let out = redact_log(log);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 5);
        assert!(lines[0].contains("iniciando"));
        assert_eq!(lines[1], REDACTED_LINE);
        assert_eq!(lines[2], REDACTED_LINE);
        assert!(lines[3].contains("bloco transcrito"), "sem texto, fica");
        assert!(lines[4].contains("ditado falhou"));
        assert!(!out.contains("segredo"));
    }

    #[test]
    fn recent_logs_pega_os_mais_novos_pelo_nome() {
        let dir = std::env::temp_dir().join(format!("isper-data-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for day in ["2026-09-01", "2026-09-10", "2026-09-11", "2026-09-12"] {
            std::fs::write(dir.join(format!("isper.log.{day}")), "x").unwrap();
        }
        std::fs::write(dir.join("outro.txt"), "x").unwrap();
        let logs = recent_logs(&dir, 3);
        let names: Vec<String> = logs
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec![
                "isper.log.2026-09-12",
                "isper.log.2026-09-11",
                "isper.log.2026-09-10"
            ]
        );
        assert!(recent_logs(&dir.join("nao-existe"), 3).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_meeting_files_apaga_md_e_exportacoes_e_tolera_ausencia() {
        let dir = std::env::temp_dir().join(format!("isper-data-rm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let md = dir.join("reuniao-20260901-100000.md");
        std::fs::write(&md, "#").unwrap();
        std::fs::write(md.with_extension("srt"), "1").unwrap();
        assert_eq!(remove_meeting_files(&md), 2, "md + srt; sem docx");
        assert!(!md.exists() && !md.with_extension("srt").exists());
        assert_eq!(remove_meeting_files(&md), 0, "já não há nada — sem erro");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_backups_deixa_os_mais_novos_e_nao_toca_nos_manuais() {
        let dir = std::env::temp_dir().join(format!("isper-data-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for stamp in [
            "20260901-100000",
            "20260905-100000",
            "20260910-100000",
            "20260913-100000",
        ] {
            std::fs::write(
                dir.join(format!("{RETENTION_BACKUP_PREFIX}{stamp}.db")),
                "x",
            )
            .unwrap();
        }
        std::fs::write(dir.join("isper-20260801-090000.db"), "manual").unwrap();
        prune_backups(&dir, RETENTION_BACKUP_PREFIX, 3);
        let mut left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            vec![
                "isper-20260801-090000.db",
                "isper-antes-da-retencao-20260905-100000.db",
                "isper-antes-da-retencao-20260910-100000.db",
                "isper-antes-da-retencao-20260913-100000.db",
            ]
        );
        prune_backups(&dir.join("nao-existe"), RETENTION_BACKUP_PREFIX, 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_now_ts_esta_no_relogio_das_datas_do_app() {
        // A data de hoje formatada como o app grava, lida de volta pelo store,
        // tem de estar a menos de um minuto de `local_now_ts`.
        let stamp = chrono::Local::now().format("%d/%m/%Y %H:%M:%S").to_string();
        let parsed = isper_core::store::parse_local_stamp(&stamp).unwrap();
        assert!((parsed - local_now_ts()).abs() < 60);
    }
}
