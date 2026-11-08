pub mod actor;
pub mod builtin;
pub mod discover;
pub mod process;
pub mod uci;

use crate::db::Db;
use crate::paths::now;
use actor::EngineHandle;
use anyhow::{anyhow, Context, Result};
use process::{EngineIdentity, Process};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;
use uci::UciOption;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineRecord {
    pub id: String,
    pub name: String,
    pub path: String,
    pub author: Option<String>,
    /// User-chosen UCI option values, applied every time the engine starts.
    pub options: BTreeMap<String, String>,
    pub available_options: Vec<UciOption>,
    /// Installed by OUTPOST into its data directory (and removable by it).
    pub managed: bool,
    /// The engine that ships with OUTPOST (Stockfish 19 Lite). Cannot be removed.
    pub builtin: bool,
    pub created_at: i64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineDefaults {
    pub threads: u32,
    pub hash_mb: u32,
}

impl Default for EngineDefaults {
    fn default() -> Self {
        let cores = std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(2);
        // Small by default: the built-in engine needs ~10 MB plus its hash table.
        // Leave headroom for the UI and the rest of the desktop.
        EngineDefaults { threads: (cores / 2).clamp(1, 4), hash_mb: 16 }
    }
}

fn row_to_record(row: &rusqlite::Row) -> rusqlite::Result<EngineRecord> {
    let options: String = row.get(4)?;
    let available: String = row.get(5)?;
    Ok(EngineRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        path: row.get(2)?,
        author: row.get(3)?,
        options: serde_json::from_str(&options).unwrap_or_default(),
        available_options: serde_json::from_str(&available).unwrap_or_default(),
        managed: row.get::<_, i64>(6)? == 1,
        builtin: row.get::<_, i64>(6)? == 2,
        created_at: row.get(7)?,
    })
}

const COLUMNS: &str = "id,name,path,author,options,available_options,managed,created_at";

pub fn list(db: &Db) -> Result<Vec<EngineRecord>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM engines ORDER BY created_at, name"))?;
    let rows = stmt.query_map([], row_to_record)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get(db: &Db, id: &str) -> Result<EngineRecord> {
    db.conn()
        .query_row(&format!("SELECT {COLUMNS} FROM engines WHERE id=?1"), [id], row_to_record)
        .optional()?
        .ok_or_else(|| anyhow!("That engine is no longer registered."))
}

pub fn default_id(db: &Db) -> Option<String> {
    let configured = db.setting("default_engine");
    let all = list(db).ok()?;
    configured
        .filter(|id| all.iter().any(|e| &e.id == id))
        .or_else(|| all.iter().find(|e| e.builtin).or(all.first()).map(|e| e.id.clone()))
}

/// Registers an engine after a successful UCI handshake. Re-registering the same
/// path refreshes its identity and options but keeps user settings.
pub fn register(db: &Db, path: &Path, identity: &EngineIdentity, managed: bool) -> Result<EngineRecord> {
    let path_str = path.to_string_lossy().to_string();
    let available = serde_json::to_string(&identity.options)?;
    let conn = db.conn();
    let existing: Option<String> = conn.query_row("SELECT id FROM engines WHERE path=?1", [&path_str], |r| r.get(0)).optional()?;
    let id = match existing {
        Some(id) => {
            conn.execute(
                "UPDATE engines SET name=?2, author=?3, available_options=?4, managed=MAX(managed, ?5) WHERE id=?1",
                params![id, identity.name, identity.author, available, managed as i64],
            )?;
            id
        }
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO engines(id,name,path,author,options,available_options,managed,created_at) VALUES(?1,?2,?3,?4,'{}',?5,?6,?7)",
                params![id, identity.name, path_str, identity.author, available, managed as i64, now()],
            )?;
            id
        }
    };
    drop(conn);
    get(db, &id)
}

pub fn set_options(db: &Db, id: &str, options: &BTreeMap<String, String>) -> Result<()> {
    let record = get(db, id)?;
    // Only keep options the engine actually declared; values are sanitised again on send.
    let filtered: BTreeMap<String, String> = options
        .iter()
        .filter(|(name, _)| record.available_options.iter().any(|o| o.name.eq_ignore_ascii_case(name) && o.kind != "button"))
        .filter(|(name, _)| !is_reserved_option(name))
        .map(|(k, v)| (uci::sanitize(k), uci::sanitize(v)))
        .collect();
    db.conn().execute("UPDATE engines SET options=?2 WHERE id=?1", params![id, serde_json::to_string(&filtered)?])?;
    Ok(())
}

pub fn remove(db: &Db, id: &str) -> Result<EngineRecord> {
    let record = get(db, id)?;
    db.conn().execute("DELETE FROM engines WHERE id=?1", [id])?;
    Ok(record)
}

/// Options OUTPOST controls itself.
fn is_reserved_option(name: &str) -> bool {
    ["MultiPV", "UCI_Chess960", "UCI_AnalyseMode", "Ponder", "UCI_ShowWDL"].iter().any(|r| r.eq_ignore_ascii_case(name))
}

pub fn startup_options(record: &EngineRecord, identity: &EngineIdentity, defaults: &EngineDefaults) -> Vec<(String, Option<String>)> {
    let declared = |name: &str| identity.options.iter().find(|o| o.name.eq_ignore_ascii_case(name)).map(|o| o.name.clone());
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    let user = |name: &str| record.options.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone());
    if let Some(name) = declared("Threads") {
        out.push((name, Some(user("Threads").unwrap_or_else(|| defaults.threads.to_string()))));
    }
    if let Some(name) = declared("Hash") {
        out.push((name, Some(user("Hash").unwrap_or_else(|| defaults.hash_mb.to_string()))));
    }
    for (name, value) in &record.options {
        if name.eq_ignore_ascii_case("Threads") || name.eq_ignore_ascii_case("Hash") || is_reserved_option(name) {
            continue;
        }
        if let Some(declared) = declared(name) {
            out.push((declared, Some(value.clone())));
        }
    }
    for flag in ["UCI_AnalyseMode", "UCI_ShowWDL"] {
        if let Some(name) = declared(flag) {
            out.push((name, Some("true".into())));
        }
    }
    out
}

/// Starts a fresh engine process with the record's options. Not shared.
pub async fn launch(record: &EngineRecord, defaults: &EngineDefaults) -> Result<EngineHandle> {
    let path = PathBuf::from(&record.path);
    let mut process = Process::spawn(&path).await?;
    let identity = process
        .handshake(Duration::from_secs(10))
        .await
        .with_context(|| format!("{} did not start as a UCI engine", record.name))?;
    let options = startup_options(record, &identity, defaults);
    actor::start(record.id.clone(), process, identity, options).await
}

/// Long-lived engines used for interactive analysis, at most one process each.
#[derive(Default)]
pub struct EngineManager {
    running: tokio::sync::Mutex<HashMap<String, EngineHandle>>,
}

impl EngineManager {
    pub async fn handle(&self, record: &EngineRecord, defaults: &EngineDefaults) -> Result<EngineHandle> {
        let mut running = self.running.lock().await;
        if let Some(handle) = running.get(&record.id) {
            if handle.is_alive() {
                return Ok(handle.clone());
            }
        }
        let handle = launch(record, defaults).await?;
        running.insert(record.id.clone(), handle.clone());
        Ok(handle)
    }

    pub async fn running(&self, id: &str) -> Option<EngineHandle> {
        self.running.lock().await.get(id).filter(|h| h.is_alive()).cloned()
    }

    pub async fn running_ids(&self) -> Vec<String> {
        self.running.lock().await.iter().filter(|(_, h)| h.is_alive()).map(|(id, _)| id.clone()).collect()
    }

    pub async fn stop(&self, id: &str) {
        if let Some(handle) = self.running(id).await {
            handle.stop();
        }
    }

    pub async fn shutdown(&self, id: &str) {
        let handle = self.running.lock().await.remove(id);
        if let Some(handle) = handle {
            handle.quit().await;
        }
    }

    pub async fn shutdown_all(&self) {
        let handles: Vec<EngineHandle> = self.running.lock().await.drain().map(|(_, h)| h).collect();
        futures_util::future::join_all(handles.iter().map(|h| h.quit())).await;
    }
}
