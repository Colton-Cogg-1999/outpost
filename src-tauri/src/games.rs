//! The local game library.

use crate::chess::pgn::ParsedGame;
use crate::db::Db;
use crate::paths::now;
use anyhow::{anyhow, Result};
use rusqlite::{params, params_from_iter, OptionalExtension, ToSql};
use serde::{Deserialize, Serialize};

/// Everything needed to store one game. Built from a validated `ParsedGame` plus any
/// metadata the source API provides beyond the PGN headers.
#[derive(Clone, Debug, Default)]
pub struct NewGame {
    pub source: String,
    pub source_id: Option<String>,
    pub account: Option<String>,
    pub url: Option<String>,
    pub white: String,
    pub black: String,
    pub white_elo: Option<i64>,
    pub black_elo: Option<i64>,
    pub result: String,
    pub date: Option<String>,
    pub played_at: Option<i64>,
    pub time_control: Option<String>,
    pub speed: Option<String>,
    pub event: Option<String>,
    pub site: Option<String>,
    pub eco: Option<String>,
    pub opening: Option<String>,
    pub termination: Option<String>,
    pub ply_count: i64,
    pub pgn: String,
    pub fingerprint: String,
}

impl NewGame {
    pub fn from_parsed(game: &ParsedGame, source: &str) -> NewGame {
        let h = |name: &str| game.header(name).map(str::to_string);
        let elo = |name: &str| game.header(name).and_then(|v| v.parse::<i64>().ok());
        NewGame {
            source: source.to_string(),
            source_id: None,
            account: None,
            url: h("Link").or_else(|| h("Site").filter(|s| s.starts_with("http"))),
            white: h("White").unwrap_or_else(|| "?".into()),
            black: h("Black").unwrap_or_else(|| "?".into()),
            white_elo: elo("WhiteElo"),
            black_elo: elo("BlackElo"),
            result: game.result.clone(),
            date: h("UTCDate").or_else(|| h("Date")),
            played_at: None,
            time_control: h("TimeControl"),
            speed: h("TimeControl").and_then(|tc| speed_from_time_control(&tc).map(str::to_string)),
            event: h("Event"),
            site: h("Site"),
            eco: h("ECO"),
            opening: h("Opening").or_else(|| h("ECOUrl").and_then(|u| u.rsplit('/').next().map(|s| s.replace('-', " ")))),
            termination: h("Termination"),
            ply_count: game.moves_uci.len() as i64,
            pgn: game.pgn.clone(),
            fingerprint: game.fingerprint(),
        }
    }
}

/// Lichess's speed buckets from estimated duration = base + 40 × increment (seconds).
pub fn speed_from_time_control(tc: &str) -> Option<&'static str> {
    if tc.contains('/') {
        return Some("correspondence");
    }
    let (base, inc) = tc.split_once('+').unwrap_or((tc, "0"));
    let base: f64 = base.parse().ok()?;
    let inc: f64 = inc.parse().unwrap_or(0.0);
    let total = base + 40.0 * inc;
    Some(match total {
        t if t < 30.0 => "ultrabullet",
        t if t < 180.0 => "bullet",
        t if t < 480.0 => "blitz",
        t if t < 1500.0 => "rapid",
        _ => "classical",
    })
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Inserted {
    New(i64),
    Duplicate(i64),
}

pub fn insert(db: &Db, game: &NewGame) -> Result<Inserted> {
    let conn = db.conn();
    if let Some(source_id) = &game.source_id {
        let existing: Option<i64> = conn
            .query_row("SELECT id FROM games WHERE source=?1 AND source_id=?2", params![game.source, source_id], |r| r.get(0))
            .optional()?;
        if let Some(id) = existing {
            return Ok(Inserted::Duplicate(id));
        }
    }
    let existing: Option<i64> =
        conn.query_row("SELECT id FROM games WHERE fingerprint=?1", [&game.fingerprint], |r| r.get(0)).optional()?;
    if let Some(id) = existing {
        // Same game seen through another channel: enrich it with the API identity.
        if game.source_id.is_some() {
            conn.execute(
                "UPDATE games SET source=?2, source_id=?3, account=COALESCE(account, ?4), url=COALESCE(url, ?5) WHERE id=?1 AND source_id IS NULL",
                params![id, game.source, game.source_id, game.account, game.url],
            )?;
        }
        return Ok(Inserted::Duplicate(id));
    }
    conn.execute(
        "INSERT INTO games(source,source_id,account,url,white,black,white_elo,black_elo,result,date,played_at,time_control,speed,event,site,eco,opening,termination,ply_count,pgn,fingerprint,imported_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22)",
        params![
            game.source, game.source_id, game.account, game.url, game.white, game.black, game.white_elo, game.black_elo,
            game.result, game.date, game.played_at, game.time_control, game.speed, game.event, game.site, game.eco,
            game.opening, game.termination, game.ply_count, game.pgn, game.fingerprint, now()
        ],
    )?;
    Ok(Inserted::New(conn.last_insert_rowid()))
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSummary {
    pub id: i64,
    pub source: String,
    pub account: Option<String>,
    pub url: Option<String>,
    pub white: String,
    pub black: String,
    pub white_elo: Option<i64>,
    pub black_elo: Option<i64>,
    pub result: String,
    pub date: Option<String>,
    pub played_at: Option<i64>,
    pub time_control: Option<String>,
    pub speed: Option<String>,
    pub event: Option<String>,
    pub eco: Option<String>,
    pub opening: Option<String>,
    pub ply_count: i64,
    pub favorite: bool,
    pub analyzed: bool,
    pub imported_at: i64,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GameFilter {
    pub search: Option<String>,
    pub source: Option<String>,
    pub speed: Option<String>,
    pub result: Option<String>,
    pub player: Option<String>,
    pub favorites: bool,
    pub analyzed: Option<bool>,
    /// "played" (default) | "imported" | "white" | "black" | "moves"
    pub sort: Option<String>,
    pub ascending: bool,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

const SUMMARY: &str = "g.id,g.source,g.account,g.url,g.white,g.black,g.white_elo,g.black_elo,g.result,g.date,g.played_at,g.time_control,g.speed,g.event,g.eco,g.opening,g.ply_count,g.favorite,(a.game_id IS NOT NULL),g.imported_at";

fn summary(row: &rusqlite::Row) -> rusqlite::Result<GameSummary> {
    Ok(GameSummary {
        id: row.get(0)?,
        source: row.get(1)?,
        account: row.get(2)?,
        url: row.get(3)?,
        white: row.get(4)?,
        black: row.get(5)?,
        white_elo: row.get(6)?,
        black_elo: row.get(7)?,
        result: row.get(8)?,
        date: row.get(9)?,
        played_at: row.get(10)?,
        time_control: row.get(11)?,
        speed: row.get(12)?,
        event: row.get(13)?,
        eco: row.get(14)?,
        opening: row.get(15)?,
        ply_count: row.get(16)?,
        favorite: row.get::<_, i64>(17)? != 0,
        analyzed: row.get(18)?,
        imported_at: row.get(19)?,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GamePage {
    pub games: Vec<GameSummary>,
    pub total: i64,
}

pub fn list(db: &Db, filter: &GameFilter) -> Result<GamePage> {
    let mut clauses: Vec<String> = Vec::new();
    let mut args: Vec<Box<dyn ToSql>> = Vec::new();
    if let Some(search) = filter.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let like = format!("%{}%", search.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        clauses.push("(g.white LIKE ? ESCAPE '\\' OR g.black LIKE ? ESCAPE '\\' OR g.event LIKE ? ESCAPE '\\' OR g.opening LIKE ? ESCAPE '\\' OR g.eco LIKE ? ESCAPE '\\')".into());
        for _ in 0..5 {
            args.push(Box::new(like.clone()));
        }
    }
    if let Some(source) = filter.source.as_deref().filter(|s| !s.is_empty()) {
        clauses.push("g.source = ?".into());
        args.push(Box::new(source.to_string()));
    }
    if let Some(speed) = filter.speed.as_deref().filter(|s| !s.is_empty()) {
        clauses.push("g.speed = ?".into());
        args.push(Box::new(speed.to_string()));
    }
    if let Some(result) = filter.result.as_deref().filter(|s| !s.is_empty()) {
        clauses.push("g.result = ?".into());
        args.push(Box::new(result.to_string()));
    }
    if let Some(player) = filter.player.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        clauses.push("(g.white = ? COLLATE NOCASE OR g.black = ? COLLATE NOCASE)".into());
        args.push(Box::new(player.to_string()));
        args.push(Box::new(player.to_string()));
    }
    if filter.favorites {
        clauses.push("g.favorite = 1".into());
    }
    match filter.analyzed {
        Some(true) => clauses.push("a.game_id IS NOT NULL".into()),
        Some(false) => clauses.push("a.game_id IS NULL".into()),
        None => {}
    }
    let filter_sql = if clauses.is_empty() { String::new() } else { format!("WHERE {}", clauses.join(" AND ")) };
    let order = match filter.sort.as_deref() {
        Some("imported") => "g.imported_at",
        Some("white") => "g.white COLLATE NOCASE",
        Some("black") => "g.black COLLATE NOCASE",
        Some("moves") => "g.ply_count",
        _ => "COALESCE(g.played_at, g.imported_at)",
    };
    let direction = if filter.ascending { "ASC" } else { "DESC" };
    let limit = filter.limit.unwrap_or(200).clamp(1, 1000);
    let offset = filter.offset.unwrap_or(0).max(0);
    let conn = db.conn();
    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM games g LEFT JOIN analyses a ON a.game_id=g.id {filter_sql}"),
        params_from_iter(args.iter().map(|a| a.as_ref())),
        |r| r.get(0),
    )?;
    let sql = format!("SELECT {SUMMARY} FROM games g LEFT JOIN analyses a ON a.game_id=g.id {filter_sql} ORDER BY {order} {direction}, g.id DESC LIMIT {limit} OFFSET {offset}");
    let mut stmt = conn.prepare(&sql)?;
    let games = stmt.query_map(params_from_iter(args.iter().map(|a| a.as_ref())), summary)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(GamePage { games, total })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameDetail {
    #[serde(flatten)]
    pub summary: GameSummary,
    pub pgn: String,
}

pub fn get(db: &Db, id: i64) -> Result<GameDetail> {
    let conn = db.conn();
    let row = conn
        .query_row(&format!("SELECT {SUMMARY}, g.pgn FROM games g LEFT JOIN analyses a ON a.game_id=g.id WHERE g.id=?1"), [id], |r| {
            Ok(GameDetail { summary: summary(r)?, pgn: r.get(20)? })
        })
        .optional()?;
    row.ok_or_else(|| anyhow!("Game {id} is not in the library."))
}

pub fn update_pgn(db: &Db, id: i64, game: &ParsedGame) -> Result<()> {
    db.conn().execute(
        "UPDATE games SET pgn=?2, ply_count=?3, result=?4 WHERE id=?1",
        params![id, game.pgn, game.moves_uci.len() as i64, game.result],
    )?;
    Ok(())
}

pub fn set_favorite(db: &Db, id: i64, favorite: bool) -> Result<()> {
    db.conn().execute("UPDATE games SET favorite=?2 WHERE id=?1", params![id, favorite as i64])?;
    Ok(())
}

pub fn delete(db: &Db, id: i64) -> Result<()> {
    db.conn().execute("DELETE FROM games WHERE id=?1", [id])?;
    Ok(())
}

#[derive(Debug, Default, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub imported: usize,
    pub duplicates: usize,
    pub failed: usize,
    pub errors: Vec<String>,
    pub first_id: Option<i64>,
}

/// Imports every game in a PGN text. Games with an illegal move are imported up to
/// that move and reported; games with no legal moves and no headers are skipped.
pub fn import_pgn_text(db: &Db, text: &str, source: &str) -> Result<ImportSummary> {
    let mut summary = ImportSummary::default();
    for (index, game) in crate::chess::pgn::parse_many(text, 20_000).into_iter().enumerate() {
        if let Some(error) = &game.error {
            if summary.errors.len() < 20 {
                summary.errors.push(format!("Game {}: {error}", index + 1));
            }
            if game.moves_uci.is_empty() {
                summary.failed += 1;
                continue;
            }
        }
        match insert(db, &NewGame::from_parsed(&game, source))? {
            Inserted::New(id) => {
                summary.imported += 1;
                summary.first_id.get_or_insert(id);
            }
            Inserted::Duplicate(id) => {
                summary.duplicates += 1;
                summary.first_id.get_or_insert(id);
            }
        }
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO: &str = "[White \"A\"]\n[Black \"B\"]\n[Date \"2024.01.01\"]\n[TimeControl \"180+2\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Qh5 Nc6 3. Bc4 Nf6 4. Qxf7# 1-0\n\n[White \"C\"]\n[Black \"D\"]\n[Result \"*\"]\n\n1. d4 *\n";

    #[test]
    fn imports_and_deduplicates() {
        let db = Db::in_memory().unwrap();
        let first = import_pgn_text(&db, TWO, "pgn").unwrap();
        assert_eq!((first.imported, first.duplicates), (2, 0));
        let second = import_pgn_text(&db, TWO, "pgn").unwrap();
        assert_eq!((second.imported, second.duplicates), (0, 2));
        let page = list(&db, &GameFilter::default()).unwrap();
        assert_eq!(page.total, 2);
        let a = page.games.iter().find(|g| g.white == "A").unwrap();
        assert_eq!(a.speed.as_deref(), Some("blitz"));
        assert_eq!(a.ply_count, 7);
        assert_eq!(a.result, "1-0");
    }

    #[test]
    fn api_source_id_dedups_and_enriches() {
        let db = Db::in_memory().unwrap();
        import_pgn_text(&db, TWO, "pgn").unwrap();
        let parsed = crate::chess::pgn::parse_one(TWO).unwrap();
        let mut game = NewGame::from_parsed(&parsed, "lichess");
        game.source_id = Some("abcd1234".into());
        assert!(matches!(insert(&db, &game).unwrap(), Inserted::Duplicate(_)));
        assert!(matches!(insert(&db, &game).unwrap(), Inserted::Duplicate(_)));
        let page = list(&db, &GameFilter { source: Some("lichess".into()), ..Default::default() }).unwrap();
        assert_eq!(page.total, 1, "the pasted copy was upgraded with its API identity");
    }

    #[test]
    fn filters_search_and_sort() {
        let db = Db::in_memory().unwrap();
        import_pgn_text(&db, TWO, "pgn").unwrap();
        let page = list(&db, &GameFilter { search: Some("c".into()), ..Default::default() }).unwrap();
        assert_eq!(page.total, 1);
        let page = list(&db, &GameFilter { player: Some("a".into()), ..Default::default() }).unwrap();
        assert_eq!(page.total, 1);
        let page = list(&db, &GameFilter { sort: Some("moves".into()), ascending: true, ..Default::default() }).unwrap();
        assert_eq!(page.games[0].white, "C");
        let page = list(&db, &GameFilter { search: Some("%".into()), ..Default::default() }).unwrap();
        assert_eq!(page.total, 0, "LIKE wildcards are escaped");
        let id = page.games.first().map(|g| g.id).unwrap_or(1);
        set_favorite(&db, id, true).unwrap();
        assert_eq!(list(&db, &GameFilter { favorites: true, ..Default::default() }).unwrap().total, 1);
        delete(&db, id).unwrap();
        assert_eq!(list(&db, &GameFilter::default()).unwrap().total, 1);
    }

    #[test]
    fn speed_buckets() {
        assert_eq!(speed_from_time_control("60"), Some("bullet"));
        assert_eq!(speed_from_time_control("180+2"), Some("blitz"));
        assert_eq!(speed_from_time_control("600"), Some("rapid"));
        assert_eq!(speed_from_time_control("1800+30"), Some("classical"));
        assert_eq!(speed_from_time_control("1/86400"), Some("correspondence"));
        assert_eq!(speed_from_time_control("-"), None);
    }
}
