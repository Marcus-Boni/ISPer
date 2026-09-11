//! Prelúdio interno: reexporta os módulos do app e os tipos externos usados em
//! toda parte, para que cada módulo comece com um único `use crate::prelude::*;`.

pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
pub(crate) use std::time::{Duration, Instant};

pub(crate) use serde_json::json;
pub(crate) use tauri::{AppHandle, Emitter, Manager};

pub(crate) use crate::calls::*;
pub(crate) use crate::dictation::*;
pub(crate) use crate::home::*;
pub(crate) use crate::insights::*;
pub(crate) use crate::library::*;
pub(crate) use crate::meetings::*;
pub(crate) use crate::overlay::*;
pub(crate) use crate::search::*;
pub(crate) use crate::settings::*;
pub(crate) use crate::shortcuts::*;
pub(crate) use crate::state::*;
pub(crate) use crate::tray::*;
pub(crate) use crate::updater::*;
pub(crate) use crate::views::*;
pub(crate) use crate::{config, notify};

/// `Mutex::lock()` sem `unwrap()`: um mutex envenenado (uma thread entrou em
/// pânico com ele travado) não derruba o app inteiro — o pânico original já
/// foi para o log pelo hook de `isper_core::panics`, e o estado protegido
/// continua utilizável (no pior caso, uma preferência fica desatualizada).
/// Derrubar o app da bandeja por causa disso custaria a reunião em andamento.
pub(crate) trait LockExt<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> LockExt<T> for Mutex<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
