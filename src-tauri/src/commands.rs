//! Every native capability the interface can use. Inputs are validated here or in the
//! module they call; nothing here builds a shell command or touches arbitrary paths.

use crate::ai::{self, context, AiEvent, AiSettings, Message};
use crate::analysis::review::{self, GameReport, ReviewEvent, ReviewSettings};
use crate::chess::{self, pgn};
use crate::db::Db;
use crate::engine::actor::{SearchEvent, SearchRequest};
use crate::engine::{self, discover, EngineDefaults, EngineManager, EngineRecord};
use crate::tactics;
use crate::error::{fail, CmdResult};
use crate::games::{self, GameFilter, ImportSummary};
use crate::import::{chesscom, lichess, ImportProgress};
use crate::paths::{now, Paths};
use crate::secrets;
use rusqlite::params;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::mpsc;

pub struct AppState {
    pub paths: Paths,
    pub db: Arc<Db>,
    pub engines: EngineManager,
    pub http: reqwest::Client,
    pub jobs: Jobs,
}

/// Cancellation for long-running work. The interface picks the job id, so it can
/// cancel a job before the starting command has even returned.
#[derive(Default)]
pub struct Jobs {
    flags: Mutex<HashMap<u64, Arc<AtomicBool>>>,
    aborts: Mutex<HashMap<u64, tokio::task::AbortHandle>>,
}

impl Jobs {
    fn start(&self, id: u64) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.flags.lock().unwrap().insert(id, flag.clone());
        flag
    }
    fn finish(&self, id: u64) {
        self.flags.lock().unwrap().remove(&id);
        self.aborts.lock().unwrap().remove(&id);
    }
    fn cancel(&self, id: u64) {
        if let Some(flag) = self.flags.lock().unwrap().get(&id) {
            flag.store(true, Ordering::Relaxed);
        }
        if let Some(handle) = self.aborts.lock().unwrap().remove(&id) {
            handle.abort();
        }
    }
}

fn engine_defaults(db: &Db) -> EngineDefaults {
    db.setting_json("engineDefaults").unwrap_or_default()
}

fn ai_settings(db: &Db) -> AiSettings {
    db.setting_json("ai").unwrap_or_default()
}

// ───────────────────────────── app & settings ─────────────────────────────

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> Value {
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "dataDir": state.paths.data,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "cores": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        "engineDefaults": engine_defaults(&state.db),
    })
}

const SETTING_KEYS: &[&str] = &["ui", "engineDefaults", "review", "ai", "analysis"];

#[tauri::command]
pub fn settings_get(state: State<'_, AppState>) -> Value {
    let mut out = serde_json::Map::new();
    for key in SETTING_KEYS {
        if let Some(value) = state.db.setting_json::<Value>(key) {
            out.insert(key.to_string(), value);
        }
    }
    out.insert("engineDefaults".into(), json!(engine_defaults(&state.db)));
    out.insert("review".into(), json!(state.db.setting_json::<ReviewSettings>("review").unwrap_or_default()));
    out.insert("ai".into(), json!(ai_settings(&state.db)));
    Value::Object(out)
}

#[tauri::command]
pub fn settings_set(state: State<'_, AppState>, key: String, value: Value) -> CmdResult<()> {
    match key.as_str() {
        "engineDefaults" => {
            let mut d: EngineDefaults = serde_json::from_value(value)?;
            d.threads = d.threads.clamp(1, 1024);
            d.hash_mb = d.hash_mb.clamp(1, 1 << 20);
            state.db.set_setting_json(&key, &d)?;
        }
        "review" => {
            let r: ReviewSettings = serde_json::from_value(value)?;
            state.db.set_setting_json(&key, &r)?;
        }
        "ai" => {
            let mut a: AiSettings = serde_json::from_value(value)?;
            if let Some(active) = &a.active {
                if ai::provider(active).is_none() {
                    return fail("Unknown AI provider.");
                }
            }
            for config in a.providers.values_mut() {
                if let Some(url) = config.base_url.as_deref().filter(|u| !u.trim().is_empty()) {
                    config.base_url = Some(ai::check_base_url(url)?);
                }
            }
            state.db.set_setting_json(&key, &a)?;
        }
        "ui" | "analysis" => {
            if value.to_string().len() > 64 * 1024 {
                return fail("Setting too large.");
            }
            state.db.set_setting_json(&key, &value)?;
        }
        _ => return fail("Unknown setting."),
    }
    Ok(())
}

#[tauri::command]
pub fn job_cancel(state: State<'_, AppState>, job_id: u64) {
    state.jobs.cancel(job_id);
}

// ───────────────────────────── chess & files ─────────────────────────────

#[tauri::command]
pub fn fen_check(fen: String) -> Value {
    match chess::parse_fen(&fen) {
        Ok(pos) => json!({"ok": true, "fen": chess::to_fen(&pos), "status": chess::status(&pos)}),
        Err(e) => json!({"ok": false, "error": e.to_string()}),
    }
}

#[tauri::command]
pub fn pgn_parse(text: String, limit: Option<usize>) -> CmdResult<Vec<pgn::ParsedGame>> {
    if text.len() > 100 * 1024 * 1024 {
        return fail("That PGN is too large to open at once (over 100 MB).");
    }
    Ok(pgn::parse_many(&text, limit.unwrap_or(2000).min(20_000)))
}

fn checked_path(path: &str, extensions: &[&str]) -> CmdResult<PathBuf> {
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return fail("Expected an absolute file path.");
    }
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    if !extensions.contains(&ext.as_str()) {
        return fail(format!("Unsupported file type '.{ext}'. Expected one of: {}.", extensions.join(", ")));
    }
    Ok(path)
}

#[tauri::command]
pub async fn file_read_text(path: String) -> CmdResult<String> {
    let path = checked_path(&path, &["pgn", "fen", "epd", "txt"])?;
    let meta = tokio::fs::metadata(&path).await?;
    if meta.len() > 100 * 1024 * 1024 {
        return fail("File is larger than 100 MB.");
    }
    let bytes = tokio::fs::read(&path).await?;
    // PGN files from older software are often Latin-1; decode leniently.
    Ok(match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => e.into_bytes().iter().map(|&b| b as char).collect(),
    })
}

#[tauri::command]
pub async fn file_write_text(path: String, contents: String) -> CmdResult<()> {
    let path = checked_path(&path, &["pgn", "fen", "txt"])?;
    tokio::fs::write(&path, contents).await?;
    Ok(())
}

#[tauri::command]
pub async fn file_read_image(path: String) -> CmdResult<tauri::ipc::Response> {
    let path = checked_path(&path, &["png", "jpg", "jpeg", "webp", "bmp", "gif"])?;
    let meta = tokio::fs::metadata(&path).await?;
    if meta.len() > 25 * 1024 * 1024 {
        return fail("Image is larger than 25 MB.");
    }
    Ok(tauri::ipc::Response::new(tokio::fs::read(&path).await?))
}

// ───────────────────────────── engines ─────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineView {
    #[serde(flatten)]
    record: EngineRecord,
    running: bool,
    default: bool,
    exists: bool,
}

#[tauri::command]
pub async fn engines_list(state: State<'_, AppState>) -> CmdResult<Vec<EngineView>> {
    let running = state.engines.running_ids().await;
    let default = engine::default_id(&state.db);
    Ok(engine::list(&state.db)?
        .into_iter()
        .map(|record| EngineView {
            running: running.contains(&record.id),
            default: default.as_deref() == Some(record.id.as_str()),
            exists: Path::new(&record.path).is_file(),
            record,
        })
        .collect())
}

#[tauri::command]
pub fn engines_detect(state: State<'_, AppState>) -> CmdResult<Vec<discover::Candidate>> {
    let registered: Vec<String> = engine::list(&state.db)?.into_iter().map(|e| e.path).collect();
    let mut extra = vec![];
    if let Ok(entries) = std::fs::read_dir(&state.paths.engines) {
        extra.extend(entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
    }
    Ok(discover::find(&discover::search_dirs(&extra)).into_iter().filter(|c| !registered.contains(&c.path)).collect())
}

#[tauri::command]
pub async fn engine_add(state: State<'_, AppState>, path: String) -> CmdResult<EngineRecord> {
    let path = PathBuf::from(path.trim());
    if !path.is_absolute() {
        return fail("Choose the engine's executable file.");
    }
    let identity = engine::process::probe(&path).await.map_err(|e| anyhow::anyhow!("{} is not a working UCI engine: {e:#}", path.display()))?;
    let managed = path.starts_with(&state.paths.engines);
    let record = engine::register(&state.db, &path, &identity, managed)?;
    if state.db.setting("default_engine").is_none() {
        state.db.set_setting("default_engine", &record.id)?;
    }
    Ok(record)
}

#[tauri::command]
pub async fn engine_remove(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    if engine::get(&state.db, &id)?.builtin {
        return fail("The built-in engine is part of OUTPOST and can't be removed. Add another engine and make it the default instead.");
    }
    state.engines.shutdown(&id).await;
    let record = engine::remove(&state.db, &id)?;
    if record.managed {
        // Only ever delete inside OUTPOST's own engines directory.
        let dir = Path::new(&record.path).parent().map(Path::to_path_buf);
        if let Some(dir) = dir.filter(|d| d.starts_with(&state.paths.engines) && d != &state.paths.engines) {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
    if state.db.setting("default_engine").as_deref() == Some(id.as_str()) {
        if let Some(next) = engine::list(&state.db)?.first() {
            state.db.set_setting("default_engine", &next.id)?;
        } else {
            state.db.conn().execute("DELETE FROM settings WHERE key='default_engine'", [])?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn engine_set_options(state: State<'_, AppState>, id: String, options: BTreeMap<String, String>) -> CmdResult<EngineRecord> {
    engine::set_options(&state.db, &id, &options)?;
    // Restart so every option takes effect from a clean state.
    state.engines.shutdown(&id).await;
    Ok(engine::get(&state.db, &id)?)
}

#[tauri::command]
pub fn engine_set_default(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    engine::get(&state.db, &id)?;
    state.db.set_setting("default_engine", &id)?;
    Ok(())
}

#[tauri::command]
pub async fn engine_restart(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.engines.shutdown(&id).await;
    let record = engine::get(&state.db, &id)?;
    state.engines.handle(&record, &engine_defaults(&state.db)).await?;
    Ok(())
}

#[tauri::command]
pub async fn engine_shutdown(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.engines.shutdown(&id).await;
    Ok(())
}

// ───────────────────────────── live analysis ─────────────────────────────

fn resolve_engine(db: &Db, id: Option<String>) -> CmdResult<EngineRecord> {
    let id = match id.filter(|s| !s.is_empty()).or_else(|| engine::default_id(db)) {
        Some(id) => id,
        None => return fail("No engine is available. This build has no built-in engine (run scripts/build-engine), so add one under Engines."),
    };
    Ok(engine::get(db, &id)?)
}

#[tauri::command]
pub async fn analysis_start(state: State<'_, AppState>, engine_id: Option<String>, request: SearchRequest, on_event: Channel<SearchEvent>) -> CmdResult<String> {
    let record = resolve_engine(&state.db, engine_id)?;
    let handle = state.engines.handle(&record, &engine_defaults(&state.db)).await?;
    let engine_name = handle.identity.name.clone();
    // Serve a cached evaluation immediately, if there is one for this position.
    if let Ok(positions) = chess::replay(request.fen.as_deref(), &request.moves) {
        let pos = positions.last().unwrap();
        if let Some((depth, lines)) = review::cached_lines(&state.db, &chess::position_key(pos), &engine_name) {
            let mut lines = lines;
            lines.truncate(request.multipv.max(1) as usize);
            let snapshot = crate::engine::actor::Snapshot {
                search_id: request.search_id,
                engine: engine_name.clone(),
                fen: chess::to_fen(pos),
                depth,
                lines,
                cached: true,
                ..Default::default()
            };
            let _ = on_event.send(SearchEvent::Update { snapshot });
        }
    }
    let (tx, mut rx) = mpsc::unbounded_channel();
    handle.search(request, tx)?;
    let db = state.db.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let SearchEvent::Done { snapshot, .. } = &event {
                if snapshot.depth >= 12 && snapshot.terminal.is_none() {
                    if let Ok(pos) = chess::parse_fen(&snapshot.fen) {
                        let _ = review::store_eval(&db, &chess::position_key(&pos), &snapshot.engine, snapshot.depth, snapshot.lines.len() as u32, &snapshot.lines);
                    }
                }
            }
            if on_event.send(event).is_err() {
                break;
            }
        }
    });
    Ok(engine_name)
}

#[tauri::command]
pub async fn analysis_stop(state: State<'_, AppState>, engine_id: Option<String>) -> CmdResult<()> {
    match engine_id {
        Some(id) => state.engines.stop(&id).await,
        None => {
            for id in state.engines.running_ids().await {
                state.engines.stop(&id).await;
            }
        }
    }
    Ok(())
}

// ───────────────────────────── game review ─────────────────────────────

#[tauri::command]
pub async fn review_start(
    state: State<'_, AppState>,
    job_id: u64,
    engine_id: Option<String>,
    start_fen: Option<String>,
    moves: Vec<String>,
    game_id: Option<i64>,
    on_event: Channel<ReviewEvent>,
) -> CmdResult<GameReport> {
    let record = resolve_engine(&state.db, engine_id)?;
    let settings: ReviewSettings = state.db.setting_json("review").unwrap_or_default();
    let cancel = state.jobs.start(job_id);
    // A dedicated process, so live analysis and review never share engine state.
    let handle = match engine::launch(&record, &engine_defaults(&state.db)).await {
        Ok(h) => h,
        Err(e) => {
            state.jobs.finish(job_id);
            return Err(e.into());
        }
    };
    let result = review::run(&state.db, &handle, start_fen.as_deref(), &moves, &settings, &cancel, |event| {
        let _ = on_event.send(event);
    })
    .await;
    handle.quit().await;
    let cancelled = cancel.load(Ordering::Relaxed);
    state.jobs.finish(job_id);
    match result {
        Ok(report) => {
            if let Some(id) = game_id {
                review::save(&state.db, id, &report)?;
            }
            let _ = on_event.send(ReviewEvent::Done { report: Box::new(report.clone()) });
            Ok(report)
        }
        Err(_) if cancelled => {
            let _ = on_event.send(ReviewEvent::Cancelled);
            fail("Review cancelled.")
        }
        Err(e) => {
            let _ = on_event.send(ReviewEvent::Error { message: format!("{e:#}") });
            Err(e.into())
        }
    }
}

#[tauri::command]
pub fn review_get(state: State<'_, AppState>, game_id: i64) -> CmdResult<Option<GameReport>> {
    Ok(review::load(&state.db, game_id)?)
}

// ───────────────────────────── library ─────────────────────────────

#[tauri::command]
pub fn games_list(state: State<'_, AppState>, filter: GameFilter) -> CmdResult<games::GamePage> {
    Ok(games::list(&state.db, &filter)?)
}

#[tauri::command]
pub fn game_get(state: State<'_, AppState>, id: i64) -> CmdResult<games::GameDetail> {
    Ok(games::get(&state.db, id)?)
}

#[tauri::command]
pub async fn games_import_pgn(state: State<'_, AppState>, text: String) -> CmdResult<ImportSummary> {
    let db = state.db.clone();
    Ok(tokio::task::spawn_blocking(move || games::import_pgn_text(&db, &text, "pgn")).await??)
}

/// Saves the current game: updates an existing library game, or adds a new one.
#[tauri::command]
pub fn game_save(state: State<'_, AppState>, id: Option<i64>, pgn_text: String) -> CmdResult<i64> {
    let parsed = pgn::parse_one(&pgn_text).ok_or_else(|| anyhow::anyhow!("Nothing to save."))?;
    if let Some(error) = &parsed.error {
        return fail(format!("Not saved: {error}"));
    }
    match id {
        Some(id) => {
            games::get(&state.db, id)?;
            games::update_pgn(&state.db, id, &parsed)?;
            Ok(id)
        }
        None => match games::insert(&state.db, &games::NewGame::from_parsed(&parsed, "local"))? {
            games::Inserted::New(id) | games::Inserted::Duplicate(id) => Ok(id),
        },
    }
}

#[tauri::command]
pub fn game_delete(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    Ok(games::delete(&state.db, id)?)
}

#[tauri::command]
pub fn game_favorite(state: State<'_, AppState>, id: i64, favorite: bool) -> CmdResult<()> {
    Ok(games::set_favorite(&state.db, id, favorite)?)
}

#[tauri::command]
pub fn accounts_list(state: State<'_, AppState>) -> CmdResult<Vec<Value>> {
    let conn = state.db.conn();
    let mut stmt = conn.prepare("SELECT platform, username, last_sync, newest_game_at FROM accounts ORDER BY last_sync DESC")?;
    let rows = stmt
        .query_map([], |r| {
            Ok(json!({"platform": r.get::<_, String>(0)?, "username": r.get::<_, String>(1)?, "lastSync": r.get::<_, Option<i64>>(2)?, "newestGameAt": r.get::<_, Option<i64>>(3)?}))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[tauri::command]
pub async fn chesscom_import(state: State<'_, AppState>, job_id: u64, options: chesscom::Options, on_progress: Channel<ImportProgress>) -> CmdResult<ImportProgress> {
    let cancel = state.jobs.start(job_id);
    let result = chesscom::import(&state.db, &state.http, options, &cancel, |p| {
        let _ = on_progress.send(p.clone());
    })
    .await;
    state.jobs.finish(job_id);
    Ok(result?)
}

#[tauri::command]
pub async fn lichess_import(state: State<'_, AppState>, job_id: u64, options: lichess::Options, on_progress: Channel<ImportProgress>) -> CmdResult<ImportProgress> {
    let cancel = state.jobs.start(job_id);
    let result = lichess::import(&state.db, &state.http, options, &cancel, |p| {
        let _ = on_progress.send(p.clone());
    })
    .await;
    state.jobs.finish(job_id);
    Ok(result?)
}

#[tauri::command]
pub async fn lichess_import_game(state: State<'_, AppState>, input: String) -> CmdResult<Value> {
    let (inserted, id) = lichess::import_game(&state.db, &state.http, &input, &AtomicBool::new(false)).await?;
    Ok(json!({"id": id, "duplicate": matches!(inserted, games::Inserted::Duplicate(_))}))
}

// ───────────────────────────── tactics from your games ─────────────────────────────

#[tauri::command]
pub fn tactics_stats(state: State<'_, AppState>) -> CmdResult<tactics::Stats> {
    Ok(tactics::stats(&state.db)?)
}

#[tauri::command]
pub fn tactics_themes(state: State<'_, AppState>) -> CmdResult<Vec<(String, i64)>> {
    Ok(tactics::themes(&state.db)?)
}

#[tauri::command]
pub fn tactic_next(state: State<'_, AppState>, filter: tactics::NextFilter) -> CmdResult<Option<tactics::Tactic>> {
    Ok(tactics::next(&state.db, &filter)?)
}

#[tauri::command]
pub fn tactics_for_game(state: State<'_, AppState>, game_id: i64) -> CmdResult<Vec<tactics::Tactic>> {
    Ok(tactics::for_game(&state.db, game_id)?)
}

#[tauri::command]
pub fn tactic_check(state: State<'_, AppState>, id: i64, index: usize, uci: String) -> CmdResult<tactics::Verdict> {
    let tactic = tactics::get(&state.db, id)?;
    Ok(tactics::check_move(&tactic, index, &uci)?)
}

#[tauri::command]
pub fn tactic_attempt(state: State<'_, AppState>, id: i64, solved: bool, mistakes: i64, hints: i64, duration_ms: i64) -> CmdResult<tactics::Tactic> {
    Ok(tactics::record_attempt(&state.db, id, solved, mistakes, hints, duration_ms)?)
}

/// Reviews games and generates tactics from them, newest first, on a dedicated engine.
#[tauri::command]
pub async fn tactics_scan(
    state: State<'_, AppState>,
    job_id: u64,
    settings: tactics::ScanSettings,
    on_progress: Channel<tactics::ScanProgress>,
) -> CmdResult<tactics::ScanProgress> {
    let record = resolve_engine(&state.db, None)?;
    let review_settings: ReviewSettings = state.db.setting_json("review").unwrap_or_default();
    let ids = tactics::pending_games(&state.db, &settings)?;
    let cancel = state.jobs.start(job_id);
    let mut summary = tactics::ScanProgress { games: ids.len(), ..Default::default() };
    if ids.is_empty() {
        state.jobs.finish(job_id);
        summary.finished = true;
        return Ok(summary);
    }
    let handle = match engine::launch(&record, &engine_defaults(&state.db)).await {
        Ok(h) => h,
        Err(e) => {
            state.jobs.finish(job_id);
            return Err(e.into());
        }
    };
    let limits = tactics::generator::Limits::default();
    let mut outcome: anyhow::Result<()> = Ok(());
    for (index, id) in ids.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let label = games::get(&state.db, *id).map(|g| format!("{} – {}", g.summary.white, g.summary.black)).unwrap_or_default();
        summary.game_index = index;
        summary.game_id = *id;
        summary.label = label;
        let base = summary.clone();
        let channel = on_progress.clone();
        let found_so_far = summary.found;
        let result = tactics::scan_game(&state.db, &handle, *id, &review_settings, &limits, settings.strictness, &cancel, |stage, done, total, _| {
            let mut p = base.clone();
            p.stage = stage.to_string();
            p.done = done;
            p.total = total;
            p.found = found_so_far;
            let _ = channel.send(p);
        })
        .await;
        match result {
            Ok(n) => summary.found += n,
            Err(_) if cancel.load(Ordering::Relaxed) => break,
            Err(e) => {
                tracing::warn!("tactic scan of game {id} failed: {e:#}");
                if !handle.is_alive() {
                    outcome = Err(e);
                    break;
                }
            }
        }
    }
    handle.quit().await;
    state.jobs.finish(job_id);
    outcome?;
    summary.finished = true;
    summary.game_index = ids.len();
    let _ = on_progress.send(summary.clone());
    Ok(summary)
}

// ───────────────────────────── AI ─────────────────────────────

#[tauri::command]
pub fn ai_providers(state: State<'_, AppState>) -> Value {
    let keys: BTreeMap<&str, secrets::KeyStatus> = ai::PROVIDERS.iter().filter(|p| p.needs_key || p.id == "compatible").map(|p| (p.id, secrets::status(p.id))).collect();
    json!({"providers": ai::PROVIDERS, "keys": keys, "settings": ai_settings(&state.db)})
}

#[tauri::command]
pub async fn ai_status(state: State<'_, AppState>, provider: String) -> CmdResult<ai::ProviderStatus> {
    if ai::provider(&provider).is_none() {
        return fail("Unknown provider.");
    }
    Ok(ai::status(&provider, &ai_settings(&state.db), &state.paths.ai_cwd).await)
}

#[tauri::command]
pub fn ai_key_set(provider: String, key: String) -> CmdResult<()> {
    if ai::provider(&provider).is_none() {
        return fail("Unknown provider.");
    }
    Ok(secrets::set(&provider, &key)?)
}

#[tauri::command]
pub fn ai_key_delete(provider: String) -> CmdResult<()> {
    Ok(secrets::delete(&provider)?)
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AiStreamEvent {
    Delta { text: String },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiAnswer {
    text: String,
    moves: context::MoveCheck,
    provider: String,
    model: Option<String>,
    chat_id: Option<String>,
    cancelled: bool,
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn ai_ask(
    state: State<'_, AppState>,
    request_id: u64,
    question: String,
    label: Option<String>,
    position: context::PositionContext,
    history: Vec<Message>,
    chat_id: Option<String>,
    on_event: Channel<AiStreamEvent>,
) -> CmdResult<AiAnswer> {
    let question = question.trim().to_string();
    if question.is_empty() {
        return fail("Ask a question first.");
    }
    if question.len() > 8000 {
        return fail("That question is too long.");
    }
    let settings = ai_settings(&state.db);
    let Some(provider_id) = settings.active.clone() else {
        return fail("AI is not set up. Choose a provider in the AI section — everything else in OUTPOST works without it.");
    };
    let model = settings.config(&provider_id).model.filter(|m| !m.is_empty());
    let block = context::render(&position)?;
    let mut messages: Vec<Message> = ai::http::history(&history, 8)
        .into_iter()
        .filter(|m| (m.role == "user" || m.role == "assistant") && !m.content.is_empty())
        .collect();
    messages.push(Message { role: "user".into(), content: format!("{block}\n## QUESTION\n{question}") });
    let turn = ai::Turn { system: context::system_prompt(&settings.skill_level, &settings.length), messages, model: model.clone() };

    let (tx, mut rx) = mpsc::unbounded_channel::<AiEvent>();
    let cwd = state.paths.ai_cwd.clone();
    let run_settings = settings.clone();
    let run_provider = provider_id.clone();
    let task = tokio::spawn(async move { ai::run(&run_provider, &run_settings, &cwd, turn, tx).await });
    state.jobs.aborts.lock().unwrap().insert(request_id, task.abort_handle());
    let mut text = String::new();
    let mut failure = None;
    while let Some(event) = rx.recv().await {
        match event {
            AiEvent::Delta(delta) => {
                text.push_str(&delta);
                let _ = on_event.send(AiStreamEvent::Delta { text: delta });
            }
            AiEvent::Failed(message) => failure = Some(message),
        }
    }
    let cancelled = task.await.is_err();
    state.jobs.finish(request_id);
    if let Some(message) = failure {
        if text.is_empty() {
            return fail(message);
        }
        text.push_str(&format!("\n\n[stopped: {message}]"));
    }
    let moves = context::check_moves(&position.fen, &text).unwrap_or_default();

    let mut saved_chat = chat_id;
    if settings.save_chats && !text.is_empty() {
        let conn = state.db.conn();
        let t = now();
        let id = match saved_chat.clone() {
            Some(id) => id,
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                let title: String = label.clone().unwrap_or_else(|| question.clone()).chars().take(80).collect();
                conn.execute("INSERT INTO chats(id,title,fen,created_at,updated_at) VALUES(?1,?2,?3,?4,?4)", params![id, title, position.fen, t])?;
                id
            }
        };
        conn.execute(
            "INSERT INTO chat_messages(chat_id,role,content,fen,created_at) VALUES(?1,'user',?2,?3,?4)",
            params![id, label.unwrap_or(question), position.fen, t],
        )?;
        conn.execute(
            "INSERT INTO chat_messages(chat_id,role,content,provider,model,fen,created_at) VALUES(?1,'assistant',?2,?3,?4,?5,?6)",
            params![id, text, provider_id, model, position.fen, t],
        )?;
        conn.execute("UPDATE chats SET updated_at=?2 WHERE id=?1", params![id, t])?;
        saved_chat = Some(id);
    }
    Ok(AiAnswer { text, moves, provider: provider_id, model, chat_id: saved_chat, cancelled })
}

#[tauri::command]
pub fn ai_check_moves(fen: String, text: String) -> CmdResult<context::MoveCheck> {
    Ok(context::check_moves(&fen, &text)?)
}

#[tauri::command]
pub fn chats_list(state: State<'_, AppState>) -> CmdResult<Vec<Value>> {
    let conn = state.db.conn();
    let mut stmt = conn.prepare("SELECT id,title,fen,updated_at,(SELECT COUNT(*) FROM chat_messages m WHERE m.chat_id=c.id) FROM chats c ORDER BY updated_at DESC LIMIT 200")?;
    let rows = stmt
        .query_map([], |r| {
            Ok(json!({"id": r.get::<_, String>(0)?, "title": r.get::<_, String>(1)?, "fen": r.get::<_, Option<String>>(2)?, "updatedAt": r.get::<_, i64>(3)?, "messages": r.get::<_, i64>(4)?}))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[tauri::command]
pub fn chat_messages(state: State<'_, AppState>, id: String) -> CmdResult<Vec<Value>> {
    let conn = state.db.conn();
    let mut stmt = conn.prepare("SELECT role,content,provider,model,fen,created_at FROM chat_messages WHERE chat_id=?1 ORDER BY id")?;
    let rows = stmt
        .query_map([id], |r| {
            Ok(json!({"role": r.get::<_, String>(0)?, "content": r.get::<_, String>(1)?, "provider": r.get::<_, Option<String>>(2)?, "model": r.get::<_, Option<String>>(3)?, "fen": r.get::<_, Option<String>>(4)?, "createdAt": r.get::<_, i64>(5)?}))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[tauri::command]
pub fn chat_delete(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.db.conn().execute("DELETE FROM chats WHERE id=?1", [id])?;
    Ok(())
}
