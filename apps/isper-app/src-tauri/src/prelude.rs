//! Prelúdio interno: reexporta os módulos do app e os tipos externos usados em
//! toda parte, para que cada módulo comece com um único `use crate::prelude::*;`.

pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::sync::{Arc, Mutex};
pub(crate) use std::time::{Duration, Instant};

pub(crate) use serde_json::json;
pub(crate) use tauri::{AppHandle, Emitter, Manager};

pub(crate) use crate::dictation::*;
pub(crate) use crate::home::*;
pub(crate) use crate::library::*;
pub(crate) use crate::meetings::*;
pub(crate) use crate::overlay::*;
pub(crate) use crate::settings::*;
pub(crate) use crate::shortcuts::*;
pub(crate) use crate::state::*;
pub(crate) use crate::tray::*;
pub(crate) use crate::updater::*;
pub(crate) use crate::views::*;
pub(crate) use crate::{config, notify};
