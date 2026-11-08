//! Tactics generated from your own games.
//!
//! A scan reviews a game (every position evaluated, reusing cached evaluations and
//! saving the review), then runs the lichess-puzzler algorithm (`generator`, a Rust
//! port) over it: wherever a move handed the other side a forced win, the winning
//! line becomes a tactic, tagged with Lichess's themes (`cook`). Training uses a
//! simple spaced-repetition schedule.
//!
//! `generator.rs`, `cook.rs` and `util.rs` are AGPL-3.0-or-later (ported from
//! lichess-puzzler); this file is OUTPOST's own GPL-3.0-or-later code.

pub mod cook;
pub mod generator;
pub mod util;

use crate::analysis::review::{self, ReviewSettings};
use crate::chess::{self, pgn};
use crate::db::Db;
use crate::engine::actor::EngineHandle;
use crate::engine::uci::Score;
use crate::games;
use crate::paths::now;
use anyhow::{anyhow, bail, Result};
use generator::{Generator, Limits, Strictness, VERSION};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use shakmaty::{Chess, Color, Move, Position};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tactic {
    pub id: i64,
    pub game_id: i64,
    pub ply: usize,
    /// Position before the opponent's mistake.
    pub fen: String,
    /// The mistake, then the solution (UCI). The solver plays moves 1, 3, 5, …
    pub moves: Vec<String>,
    pub solver: String,
    pub kind: String,
    pub cp: i64,
    pub themes: Vec<String>,
    /// "you" (your chance), "opponent" (your mistake gave it to them), or None (unknown).
    pub owner: Option<String>,
    /// What was played in the game instead of the first solution move, if not found.
    pub played: Option<String>,
    pub attempts: i64,
    pub solved: i64,
    pub streak: i64,
    pub due_at: i64,
    pub white: String,
    pub black: String,
    pub date: Option<String>,
    pub url: Option<String>,
}

const TACTIC_COLUMNS: &str = "t.id,t.game_id,t.ply,t.fen,t.moves,t.solver,t.kind,t.cp,t.themes,t.owner,t.played,t.attempts,t.solved,t.streak,t.due_at,g.white,g.black,g.date,g.url";

fn row_to_tactic(r: &rusqlite::Row) -> rusqlite::Result<Tactic> {
    let moves: String = r.get(4)?;
    let themes: String = r.get(8)?;
    Ok(Tactic {
        id: r.get(0)?,
        game_id: r.get(1)?,
        ply: r.get::<_, i64>(2)? as usize,
        fen: r.get(3)?,
        moves: moves.split_whitespace().map(str::to_string).collect(),
        solver: r.get(5)?,
        kind: r.get(6)?,
        cp: r.get(7)?,
        themes: themes.split_whitespace().map(str::to_string).collect(),
        owner: r.get(9)?,
        played: r.get(10)?,
        attempts: r.get(11)?,
        solved: r.get(12)?,
        streak: r.get(13)?,
        due_at: r.get(14)?,
        white: r.get(15)?,
        black: r.get(16)?,
        date: r.get(17)?,
        url: r.get(18)?,
    })
}

pub fn get(db: &Db, id: i64) -> Result<Tactic> {
    db.conn()
        .query_row(&format!("SELECT {TACTIC_COLUMNS} FROM tactics t JOIN games g ON g.id=t.game_id WHERE t.id=?1"), [id], row_to_tactic)
        .optional()?
        .ok_or_else(|| anyhow!("That tactic no longer exists."))
}

pub fn for_game(db: &Db, game_id: i64) -> Result<Vec<Tactic>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(&format!("SELECT {TACTIC_COLUMNS} FROM tactics t JOIN games g ON g.id=t.game_id WHERE t.game_id=?1 ORDER BY t.ply"))?;
    let rows = stmt.query_map([game_id], row_to_tactic)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Which colour the library's owner played, from the game's account or any synced account.
pub fn owner_color(db: &Db, game_id: i64) -> Result<Option<Color>> {
    let conn = db.conn();
    let (white, black, account): (String, String, Option<String>) =
        conn.query_row("SELECT white, black, account FROM games WHERE id=?1", [game_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    let mut names: Vec<String> = account.into_iter().collect();
    let mut stmt = conn.prepare("SELECT username FROM accounts")?;
    names.extend(stmt.query_map([], |r| r.get::<_, String>(0))?.flatten());
    let is = |player: &str| names.iter().any(|n| n.eq_ignore_ascii_case(player));
    Ok(match (is(&white), is(&black)) {
        (true, false) => Some(Color::White),
        (false, true) => Some(Color::Black),
        _ => None,
    })
}

#[derive(Clone, Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub games: i64,
    pub scanned: i64,
    pub tactics: i64,
    pub yours: i64,
    pub missed: i64,
    pub due: i64,
    pub fresh: i64,
    pub attempts: i64,
    pub solved_first_try: i64,
    pub streak: i64,
}

pub fn stats(db: &Db) -> Result<Stats> {
    let conn = db.conn();
    let one = |sql: &str| -> rusqlite::Result<i64> { conn.query_row(sql, [], |r| r.get(0)) };
    let now = now();
    Ok(Stats {
        games: one("SELECT COUNT(*) FROM games")?,
        scanned: one(&format!("SELECT COUNT(*) FROM tactic_scans WHERE version={VERSION}"))?,
        tactics: one("SELECT COUNT(*) FROM tactics")?,
        yours: one("SELECT COUNT(*) FROM tactics WHERE owner='you'")?,
        missed: one("SELECT COUNT(*) FROM tactics WHERE owner='you' AND played IS NOT NULL")?,
        due: conn.query_row("SELECT COUNT(*) FROM tactics WHERE attempts>0 AND due_at<=?1", [now], |r| r.get(0))?,
        fresh: one("SELECT COUNT(*) FROM tactics WHERE attempts=0")?,
        attempts: one("SELECT COUNT(*) FROM tactic_attempts")?,
        solved_first_try: one("SELECT COUNT(*) FROM tactics WHERE attempts>0 AND streak>0")?,
        streak: {
            let mut stmt = conn.prepare("SELECT solved FROM tactic_attempts ORDER BY id DESC LIMIT 500")?;
            let rows: Vec<bool> = stmt.query_map([], |r| r.get::<_, i64>(0).map(|v| v != 0))?.collect::<rusqlite::Result<_>>()?;
            rows.iter().take_while(|s| **s).count() as i64
        },
    })
}

pub fn themes(db: &Db) -> Result<Vec<(String, i64)>> {
    let conn = db.conn();
    let mut stmt = conn.prepare("SELECT themes FROM tactics")?;
    let mut counts: std::collections::BTreeMap<String, i64> = Default::default();
    for themes in stmt.query_map([], |r| r.get::<_, String>(0))?.flatten() {
        for t in themes.split_whitespace() {
            *counts.entry(t.to_string()).or_default() += 1;
        }
    }
    let mut out: Vec<(String, i64)> = counts.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Ok(out)
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct NextFilter {
    /// "you" | "opponent" | "" (all)
    pub owner: String,
    pub theme: String,
    pub game_id: Option<i64>,
    /// Skip these (e.g. the one just shown).
    pub exclude: Vec<i64>,
}

/// Due reviews first (oldest due first), then new tactics (your own chances first,
/// then the ones you missed in the game), then nothing.
pub fn next(db: &Db, filter: &NextFilter) -> Result<Option<Tactic>> {
    let mut clauses = vec!["(t.attempts=0 OR t.due_at<=?1)".to_string()];
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now())];
    if !filter.owner.is_empty() {
        clauses.push(format!("t.owner=?{}", args.len() + 1));
        args.push(Box::new(filter.owner.clone()));
    }
    if !filter.theme.is_empty() {
        clauses.push(format!("(' ' || t.themes || ' ') LIKE ?{}", args.len() + 1));
        args.push(Box::new(format!("% {} %", filter.theme.replace(['%', '_'], ""))));
    }
    if let Some(game) = filter.game_id {
        clauses.push(format!("t.game_id=?{}", args.len() + 1));
        args.push(Box::new(game));
    }
    for id in filter.exclude.iter().take(50) {
        clauses.push(format!("t.id<>?{}", args.len() + 1));
        args.push(Box::new(*id));
    }
    let sql = format!(
        "SELECT {TACTIC_COLUMNS} FROM tactics t JOIN games g ON g.id=t.game_id WHERE {} \
         ORDER BY (t.attempts=0), t.due_at, (t.owner='you') DESC, (t.played IS NOT NULL) DESC, random() LIMIT 1",
        clauses.join(" AND ")
    );
    let conn = db.conn();
    Ok(conn.query_row(&sql, rusqlite::params_from_iter(args.iter().map(|a| a.as_ref())), row_to_tactic).optional()?)
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "result")]
pub enum Verdict {
    /// Right move. `reply` is the opponent's answer (None when the tactic is over).
    Correct { reply: Option<String>, solved: bool, alternative_mate: bool },
    Incorrect { expected: String },
}

/// `index` is the position in `tactic.moves` the solver is answering (1, 3, 5, …).
pub fn check_move(tactic: &Tactic, index: usize, uci: &str) -> Result<Verdict> {
    if index % 2 == 0 || index >= tactic.moves.len() {
        bail!("Not the solver's turn in this tactic.");
    }
    let positions = chess::replay(Some(&tactic.fen), &tactic.moves[..index])?;
    let pos = positions.last().expect("replay includes the start");
    let played = chess::parse_uci(pos, uci)?;
    let expected = &tactic.moves[index];
    let is_last = index + 1 == tactic.moves.len();
    if chess::uci_string(played) == *expected {
        let reply = tactic.moves.get(index + 1).cloned();
        return Ok(Verdict::Correct { solved: reply.is_none(), reply, alternative_mate: false });
    }
    // Lichess accepts any move that mates, as the final move of a mating tactic.
    let mut after = pos.clone();
    after.play_unchecked(played);
    if after.is_checkmate() && (is_last || tactic.kind == "mate") {
        return Ok(Verdict::Correct { reply: None, solved: true, alternative_mate: true });
    }
    Ok(Verdict::Incorrect { expected: expected.clone() })
}

/// Records an attempt and schedules the next review: solved cleanly pushes the tactic
/// out (1, 3, 7, 21, 60 days); a miss brings it back in ten minutes.
pub fn record_attempt(db: &Db, id: i64, solved: bool, mistakes: i64, hints: i64, duration_ms: i64) -> Result<Tactic> {
    let t = get(db, id)?;
    let clean = solved && mistakes == 0 && hints == 0;
    let streak = if clean { t.streak + 1 } else { 0 };
    const DAYS: [i64; 5] = [1, 3, 7, 21, 60];
    let due = if clean { now() + DAYS[(streak as usize - 1).min(DAYS.len() - 1)] * 86_400 } else { now() + 600 };
    {
        let conn = db.conn();
        conn.execute(
            "INSERT INTO tactic_attempts(tactic_id,solved,mistakes,hints,duration_ms,attempted_at) VALUES(?1,?2,?3,?4,?5,?6)",
            params![id, solved as i64, mistakes, hints, duration_ms.max(0), now()],
        )?;
        conn.execute(
            "UPDATE tactics SET attempts=attempts+1, solved=solved+?2, streak=?3, due_at=?4, last_at=?5 WHERE id=?1",
            params![id, clean as i64, streak, due, now()],
        )?;
    }
    get(db, id)
}

#[derive(Clone, Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub game_index: usize,
    pub games: usize,
    pub game_id: i64,
    pub label: String,
    /// "review" (evaluating positions) or "search" (checking candidate tactics)
    pub stage: String,
    pub done: usize,
    pub total: usize,
    pub found: usize,
    pub finished: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScanSettings {
    pub strictness: Strictness,
    /// Only games where an account owner is known to have played.
    pub only_mine: bool,
    /// Rescan games already scanned with this generator version.
    pub rescan: bool,
    pub max_games: usize,
    pub game_ids: Vec<i64>,
}

impl Default for ScanSettings {
    fn default() -> Self {
        ScanSettings { strictness: Strictness::Personal, only_mine: false, rescan: false, max_games: 50, game_ids: vec![] }
    }
}

/// Newest first, games not yet scanned with this generator version.
pub fn pending_games(db: &Db, settings: &ScanSettings) -> Result<Vec<i64>> {
    if !settings.game_ids.is_empty() {
        return Ok(settings.game_ids.clone());
    }
    let conn = db.conn();
    let mine = if settings.only_mine {
        " AND (g.account IS NOT NULL OR EXISTS(SELECT 1 FROM accounts a WHERE a.username=g.white COLLATE NOCASE OR a.username=g.black COLLATE NOCASE))"
    } else {
        ""
    };
    let scanned = if settings.rescan { "" } else { " AND NOT EXISTS(SELECT 1 FROM tactic_scans s WHERE s.game_id=g.id AND s.version=?1)" };
    let sql = format!("SELECT g.id FROM games g WHERE g.ply_count>=8{scanned}{mine} ORDER BY COALESCE(g.played_at, g.imported_at) DESC LIMIT ?2");
    let mut stmt = conn.prepare(&sql)?;
    let limit = settings.max_games.clamp(1, 10_000) as i64;
    let ids = if settings.rescan {
        stmt.query_map(params![limit], |r| r.get(0))?.collect::<rusqlite::Result<Vec<i64>>>()?
    } else {
        stmt.query_map(params![VERSION, limit], |r| r.get(0))?.collect::<rusqlite::Result<Vec<i64>>>()?
    };
    Ok(ids)
}

fn color_name(c: Color) -> &'static str {
    if c == Color::White { "white" } else { "black" }
}

/// Scans one game: review (cached), then search for tactics. Returns how many were saved.
pub async fn scan_game(
    db: &Db,
    engine: &EngineHandle,
    game_id: i64,
    review_settings: &ReviewSettings,
    limits: &Limits,
    strictness: Strictness,
    cancel: &AtomicBool,
    mut progress: impl FnMut(&str, usize, usize, usize),
) -> Result<usize> {
    let game = games::get(db, game_id)?;
    let parsed = pgn::parse_one(&game.pgn).ok_or_else(|| anyhow!("The game's PGN could not be read."))?;
    let moves_uci = parsed.moves_uci.clone();
    let positions = chess::replay(parsed.start_fen.as_deref(), &moves_uci)?;
    let moves: Vec<Move> = moves_uci.iter().enumerate().map(|(i, u)| chess::parse_uci(&positions[i], u)).collect::<Result<_>>()?;

    // Evaluations: reuse a saved review of this exact game, otherwise review it now.
    let existing = review::load(db, game_id)?.filter(|r| r.positions.len() == positions.len());
    let report = match existing {
        Some(r) => r,
        None => {
            let report = review::run(db, engine, parsed.start_fen.as_deref(), &moves_uci, review_settings, cancel, |event| {
                if let review::ReviewEvent::Progress { done, total, .. } = event {
                    progress("review", done, total, 0);
                }
            })
            .await?;
            review::save(db, game_id, &report)?;
            report
        }
    };
    let evals: Vec<Score> = report.positions.iter().map(|p| p.score()).collect();

    let owner = owner_color(db, game_id)?;
    let mut generator = Generator { engine, limits: limits.clone(), rules: strictness.rules(), cancel };
    let total = moves.len();
    let found = generator.analyze_game(&positions, &moves, &evals, |ply| progress("search", ply, total, 0)).await?;
    let mut saved = 0;
    for f in &found {
        if save_found(db, game_id, &positions, &moves, f, owner)? {
            saved += 1;
        }
    }
    db.conn().execute(
        "INSERT INTO tactic_scans(game_id,version,found,scanned_at) VALUES(?1,?2,?3,?4)
         ON CONFLICT(game_id) DO UPDATE SET version=excluded.version, found=excluded.found, scanned_at=excluded.scanned_at",
        params![game_id, VERSION, saved as i64, now()],
    )?;
    if cancel.load(Ordering::Relaxed) {
        bail!("cancelled");
    }
    Ok(saved)
}

/// Stores a found tactic unless the same position is already a tactic (openings repeat).
fn save_found(db: &Db, game_id: i64, positions: &[Chess], moves: &[Move], f: &generator::Found, owner: Option<Color>) -> Result<bool> {
    let start = &positions[f.ply];
    let Some(line) = util::Line::new(start.clone(), &f.moves, f.cp) else { return Ok(false) };
    let themes = cook::cook(&line).join(" ");
    let solver = line.pov();
    let key = chess::position_key(start);
    let ucis: Vec<String> = f.moves.iter().map(|m| chess::uci_string(*m)).collect();
    let played = moves.get(f.ply + 1).map(|m| chess::uci_string(*m)).filter(|p| Some(p) != ucis.get(1));
    let owner = owner.map(|c| if c == solver { "you" } else { "opponent" });
    let conn = db.conn();
    let duplicate: bool = conn
        .query_row("SELECT 1 FROM tactics WHERE position_key=?1 AND game_id<>?2 LIMIT 1", params![key, game_id], |_| Ok(()))
        .optional()?
        .is_some();
    if duplicate {
        return Ok(false);
    }
    let changed = conn.execute(
        "INSERT INTO tactics(game_id,ply,fen,moves,position_key,solver,kind,cp,themes,owner,played,version,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
         ON CONFLICT(game_id,ply) DO UPDATE SET fen=excluded.fen, moves=excluded.moves, position_key=excluded.position_key,
           solver=excluded.solver, kind=excluded.kind, cp=excluded.cp, themes=excluded.themes, owner=excluded.owner,
           played=excluded.played, version=excluded.version",
        params![game_id, f.ply as i64, chess::to_fen(start), ucis.join(" "), key, color_name(solver), f.kind, f.cp, themes, owner, played, VERSION, now()],
    )?;
    Ok(changed > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert_game(db: &Db, pgn_text: &str, account: Option<&str>) -> i64 {
        let parsed = pgn::parse_one(pgn_text).unwrap();
        let mut new = games::NewGame::from_parsed(&parsed, "pgn");
        new.account = account.map(str::to_string);
        match games::insert(db, &new).unwrap() {
            games::Inserted::New(id) | games::Inserted::Duplicate(id) => id,
        }
    }

    fn scholar(db: &Db) -> (i64, i64) {
        let game = insert_game(db, "[White \"me\"]\n[Black \"them\"]\n\n1. e4 e5 2. Bc4 Nc6 3. Qh5 Nf6 4. Qxf7# 1-0", Some("me"));
        let positions = chess::replay(None, &["e2e4", "e7e5", "f1c4", "b8c6", "d1h5", "g8f6", "h5f7"].map(String::from)).unwrap();
        let moves: Vec<Move> = ["e2e4", "e7e5", "f1c4", "b8c6", "d1h5", "g8f6", "h5f7"].iter().enumerate().map(|(i, u)| chess::parse_uci(&positions[i], u).unwrap()).collect();
        let found = generator::Found { ply: 5, moves: vec![moves[5], moves[6]], kind: "mate", cp: 999_999_999 };
        assert!(save_found(db, game, &positions, &moves, &found, owner_color(db, game).unwrap()).unwrap());
        let id: i64 = db.conn().query_row("SELECT id FROM tactics", [], |r| r.get(0)).unwrap();
        (game, id)
    }

    #[test]
    fn saves_tags_and_attributes_a_tactic() {
        let db = Db::in_memory().unwrap();
        let (_, id) = scholar(&db);
        let t = get(&db, id).unwrap();
        assert_eq!(t.moves, ["g8f6", "h5f7"]);
        assert_eq!(t.solver, "white");
        assert_eq!(t.owner.as_deref(), Some("you"));
        assert_eq!(t.played, None, "the game's move was the solution");
        assert!(t.themes.contains(&"mateIn1".to_string()), "{:?}", t.themes);
        assert!(t.themes.contains(&"attackingF2F7".to_string()), "{:?}", t.themes);
        assert!(t.themes.contains(&"oneMove".to_string()));
    }

    #[test]
    fn checks_moves_and_accepts_other_mates() {
        let db = Db::in_memory().unwrap();
        let (_, id) = scholar(&db);
        let t = get(&db, id).unwrap();
        assert!(matches!(check_move(&t, 1, "h5f7").unwrap(), Verdict::Correct { solved: true, alternative_mate: false, .. }));
        assert!(matches!(check_move(&t, 1, "c4f7").unwrap(), Verdict::Incorrect { .. }));
        assert!(check_move(&t, 0, "g8f6").is_err());
        assert!(check_move(&t, 1, "e1e5").is_err(), "illegal moves are errors");
    }

    #[test]
    fn schedules_reviews() {
        let db = Db::in_memory().unwrap();
        let (_, id) = scholar(&db);
        assert!(next(&db, &NextFilter::default()).unwrap().is_some());
        let t = record_attempt(&db, id, true, 0, 0, 3000).unwrap();
        assert_eq!(t.streak, 1);
        assert!(t.due_at > now() + 80_000);
        assert!(next(&db, &NextFilter::default()).unwrap().is_none(), "not due yet");
        let t = record_attempt(&db, id, false, 1, 0, 3000).unwrap();
        assert_eq!(t.streak, 0);
        assert!(t.due_at <= now() + 600);
        let s = stats(&db).unwrap();
        assert_eq!((s.tactics, s.yours, s.attempts), (1, 1, 2));
    }

    #[test]
    fn identical_positions_from_other_games_are_skipped() {
        let db = Db::in_memory().unwrap();
        scholar(&db);
        let other = insert_game(&db, "[White \"x\"]\n[Black \"y\"]\n[Event \"other\"]\n\n1. e4 e5 2. Bc4 Nc6 3. Qh5 Nf6 4. Qxf7# 1-0", None);
        let positions = chess::replay(None, &["e2e4", "e7e5", "f1c4", "b8c6", "d1h5", "g8f6"].map(String::from)).unwrap();
        let moves: Vec<Move> = ["e2e4", "e7e5", "f1c4", "b8c6", "d1h5", "g8f6"].iter().enumerate().map(|(i, u)| chess::parse_uci(&positions[i], u).unwrap()).collect();
        let mate = chess::parse_uci(&positions[6], "h5f7").unwrap();
        let found = generator::Found { ply: 5, moves: vec![moves[5], mate], kind: "mate", cp: 999_999_999 };
        assert!(!save_found(&db, other, &positions, &moves, &found, None).unwrap());
    }

    #[test]
    fn pending_games_skip_scanned_ones() {
        let db = Db::in_memory().unwrap();
        let (game, _) = scholar(&db);
        assert_eq!(pending_games(&db, &ScanSettings::default()).unwrap(), vec![] as Vec<i64>, "too short (7 plies) to scan");
        let long = insert_game(&db, "[White \"me\"]\n[Black \"z\"]\n\n1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 *", Some("me"));
        assert_eq!(pending_games(&db, &ScanSettings::default()).unwrap(), vec![long]);
        db.conn().execute("INSERT INTO tactic_scans(game_id,version,found,scanned_at) VALUES(?1,?2,0,0)", params![long, VERSION]).unwrap();
        assert!(pending_games(&db, &ScanSettings::default()).unwrap().is_empty());
        let _ = game;
    }
}
