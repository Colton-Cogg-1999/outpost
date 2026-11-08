//! The engine that ships with OUTPOST: Stockfish 19 Lite (see scripts/build-engine).
//! It is found next to the OUTPOST executable (how Tauri bundles sidecars and how
//! the packages install it) or, for builds from a checkout, in src-tauri/binaries.

use super::{process, register, EngineRecord};
use crate::db::Db;
use anyhow::Result;
use rusqlite::params;
use std::path::PathBuf;

pub const NAME: &str = "outpost-stockfish";

fn file_name(with_triple: bool) -> String {
    let triple = if with_triple { format!("-{}", env!("OUTPOST_TARGET")) } else { String::new() };
    format!("{NAME}{triple}{}", std::env::consts::EXE_SUFFIX)
}

pub fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.canonicalize().ok()).and_then(|p| p.parent().map(PathBuf::from)) {
        out.push(dir.join(file_name(false)));
        out.push(dir.join(file_name(true)));
        out.push(dir.join("../lib/outpost").join(file_name(false)));
    }
    out.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries").join(file_name(true)));
    out
}

pub fn locate() -> Option<PathBuf> {
    candidates().into_iter().find(|p| super::discover::is_executable(p)).and_then(|p| p.canonicalize().ok())
}

/// Registers (or refreshes) the built-in engine and makes it the default when the
/// user hasn't chosen one. Returns None when this build has no built-in engine.
pub async fn ensure_registered(db: &Db) -> Result<Option<EngineRecord>> {
    let Some(path) = locate() else { return Ok(None) };
    let identity = process::probe(&path).await?;
    // A moved installation (new path) replaces the old record instead of adding a second one.
    let path_str = path.to_string_lossy().to_string();
    let stale: Vec<String> = {
        let conn = db.conn();
        let mut stmt = conn.prepare("SELECT id FROM engines WHERE managed=2 AND path<>?1")?;
        let ids = stmt.query_map([&path_str], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
        ids
    };
    let record = register(db, &path, &identity, false)?;
    {
        let conn = db.conn();
        conn.execute("UPDATE engines SET managed=2 WHERE id=?1", [&record.id])?;
        for old in &stale {
            // Carry user options over, then drop the old record.
            conn.execute("UPDATE engines SET options=(SELECT options FROM engines WHERE id=?2) WHERE id=?1", params![record.id, old])?;
            conn.execute("DELETE FROM engines WHERE id=?1", [old])?;
            if db_setting_is(&conn, "default_engine", old) {
                conn.execute("UPDATE settings SET value=?1 WHERE key='default_engine'", [&record.id])?;
            }
        }
    }
    if db.setting("default_engine").is_none() {
        db.set_setting("default_engine", &record.id)?;
    }
    super::get(db, &record.id).map(Some)
}

fn db_setting_is(conn: &rusqlite::Connection, key: &str, value: &str) -> bool {
    conn.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| r.get::<_, String>(0)).is_ok_and(|v| v == value)
}
