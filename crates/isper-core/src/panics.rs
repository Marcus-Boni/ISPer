//! Pânicos no log. O exe do app é `windows_subsystem = "windows"`: não tem
//! stderr, então um pânico morria sem deixar rastro. O hook daqui descreve o
//! pânico (mensagem, arquivo:linha, thread e backtrace) e entrega o texto a
//! quem o instalou — o app manda para o `tracing`, que já escreve no arquivo
//! de log. O hook anterior continua sendo chamado depois (útil no terminal).

use std::backtrace::Backtrace;
use std::panic::PanicHookInfo;

/// Descrição de um pânico: uma linha com thread, mensagem e local, seguida do
/// backtrace (sempre capturado — num pânico o custo não importa).
pub fn describe(info: &PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(payload não textual)".to_string());
    let location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "local desconhecido".to_string());
    let thread = std::thread::current();
    let thread = thread.name().unwrap_or("sem nome");
    format!(
        "pânico na thread `{thread}`: {message} ({location})\nbacktrace:\n{}",
        Backtrace::force_capture()
    )
}

/// Instala o hook global de pânico: cada pânico, em qualquer thread, passa por
/// `sink` e depois pelo hook que existia antes. Chamar uma vez, no início.
pub fn install_hook(sink: impl Fn(&str) + Send + Sync + 'static) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        sink(&describe(info));
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn hook_registra_mensagem_local_e_thread() {
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        install_hook(move |text| sink.lock().unwrap().push(text.to_string()));

        let handle = std::thread::Builder::new()
            .name("thread-de-teste".into())
            .spawn(|| panic!("explodiu {}", 42))
            .expect("spawn");
        assert!(
            handle.join().is_err(),
            "a thread deve ter entrado em pânico"
        );

        let seen = seen.lock().unwrap();
        let text = seen
            .iter()
            .find(|t| t.contains("explodiu 42"))
            .expect("o pânico deveria ter passado pelo hook");
        assert!(text.contains("thread `thread-de-teste`"), "{text}");
        assert!(text.contains("panics.rs:"), "{text}");
        assert!(text.contains("backtrace:"), "{text}");
    }
}
