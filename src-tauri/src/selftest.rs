//! Diagnostics: a log file for warnings and frontend errors, and an opt-in
//! self-test mode that drives the real interface in a hidden window.
//!
//! `OUTPOST_SELFTEST=<report file>` (with `OUTPOST_SELFTEST_USER=<name>` to also
//! exercise the Lichess and Chess.com imports) runs `src/dev/selftest.ts` and exits.

use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

pub fn report_path() -> Option<PathBuf> {
    std::env::var_os("OUTPOST_SELFTEST").map(PathBuf::from).filter(|p| p.is_absolute())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelftestInfo {
    user: Option<String>,
}

#[tauri::command]
pub fn selftest_info() -> Option<SelftestInfo> {
    report_path()?;
    let user = std::env::var("OUTPOST_SELFTEST_USER").ok().filter(|u| crate::import::valid_username(u).is_ok());
    Some(SelftestInfo { user })
}

#[tauri::command]
pub fn selftest_log(line: String) {
    let Some(path) = report_path() else { return };
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

#[tauri::command]
pub fn selftest_done(app: tauri::AppHandle) {
    if report_path().is_some() {
        app.exit(0);
    }
}

/// Frontend errors land in the same log as backend warnings.
#[tauri::command]
pub fn log_frontend(level: String, message: String) {
    let message: String = message.chars().take(4000).collect();
    match level.as_str() {
        "error" => tracing::error!(target: "frontend", "{message}"),
        _ => tracing::warn!(target: "frontend", "{message}"),
    }
}
