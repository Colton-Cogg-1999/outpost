//! Lichess public API (https://lichess.org/api). Game exports are streamed as NDJSON;
//! one request at a time, and a full minute's pause after any 429, as Lichess asks.
//! Only finished games are ever imported. OUTPOST is not affiliated with Lichess.

use super::{get, valid_username, ImportProgress, NotFound};
use crate::chess::pgn;
use crate::db::Db;
use crate::games::{self, Inserted, NewGame};
use crate::paths::now;
use anyhow::{anyhow, bail, Result};
use futures_util::StreamExt;
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Deserialize)]
pub struct User {
    pub name: Option<String>,
}

#[derive(Deserialize)]
pub struct Player {
    pub user: Option<User>,
    pub rating: Option<i64>,
    #[serde(rename = "aiLevel")]
    pub ai_level: Option<i64>,
}

#[derive(Deserialize)]
pub struct Players {
    pub white: Player,
    pub black: Player,
}

#[derive(Deserialize)]
pub struct Opening {
    pub eco: Option<String>,
    pub name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: String,
    pub rated: Option<bool>,
    pub variant: Option<String>,
    pub speed: Option<String>,
    pub created_at: Option<i64>,
    pub last_move_at: Option<i64>,
    pub status: Option<String>,
    pub players: Players,
    pub opening: Option<Opening>,
    pub pgn: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Options {
    pub username: String,
    pub max_games: Option<usize>,
    /// ultraBullet, bullet, blitz, rapid, classical, correspondence (empty = all)
    pub perf_types: Vec<String>,
    /// Unix seconds.
    pub since: Option<i64>,
    pub until: Option<i64>,
    pub rated_only: bool,
    pub incremental: bool,
}

pub fn is_finished(status: Option<&str>) -> bool {
    !matches!(status, Some("created" | "started") | None)
}

pub enum Converted {
    Game(Box<NewGame>),
    Skip(&'static str),
}

pub fn convert(game: &Game, account: Option<&str>) -> Converted {
    if !is_finished(game.status.as_deref()) {
        return Converted::Skip("in progress");
    }
    if !matches!(game.variant.as_deref(), Some("standard" | "fromPosition") | None) {
        return Converted::Skip("variant");
    }
    let Some(parsed) = game.pgn.as_deref().and_then(pgn::parse_one) else { return Converted::Skip("no PGN") };
    if parsed.error.is_some() || parsed.moves_uci.is_empty() {
        return Converted::Skip("invalid PGN");
    }
    let mut new = NewGame::from_parsed(&parsed, "lichess");
    new.source_id = Some(game.id.clone());
    new.account = account.map(str::to_lowercase);
    new.url = Some(format!("https://lichess.org/{}", game.id));
    let name = |p: &Player| p.user.as_ref().and_then(|u| u.name.clone()).or_else(|| p.ai_level.map(|l| format!("Stockfish level {l}")));
    if let Some(w) = name(&game.players.white) {
        new.white = w;
    }
    if let Some(b) = name(&game.players.black) {
        new.black = b;
    }
    new.white_elo = game.players.white.rating.or(new.white_elo);
    new.black_elo = game.players.black.rating.or(new.black_elo);
    new.played_at = game.last_move_at.or(game.created_at).map(|ms| ms / 1000);
    new.speed = game.speed.clone().map(|s| if s == "ultraBullet" { "ultrabullet".into() } else { s }).or(new.speed);
    if let Some(opening) = &game.opening {
        new.eco = opening.eco.clone().or(new.eco);
        new.opening = opening.name.clone().or(new.opening);
    }
    new.termination = game.status.clone();
    Converted::Game(Box::new(new))
}

/// Accepts a bare 8-character id, a 12-character player id, or any lichess.org game URL.
pub fn parse_game_id(input: &str) -> Result<String> {
    let input = input.trim();
    let path = input
        .strip_prefix("https://")
        .or_else(|| input.strip_prefix("http://"))
        .map(|rest| rest.strip_prefix("www.").unwrap_or(rest))
        .map(|rest| {
            rest.strip_prefix("lichess.org/").ok_or_else(|| anyhow!("That is not a lichess.org link."))
        })
        .transpose()?
        .unwrap_or_else(|| {
            let bare = input.strip_prefix("www.").unwrap_or(input);
            bare.strip_prefix("lichess.org/").unwrap_or(bare)
        });
    let segments: Vec<&str> = path.split(['/', '?', '#']).filter(|s| !s.is_empty()).collect();
    let candidate = match segments.as_slice() {
        ["game", "export", id, ..] => *id,
        [id, ..] => *id,
        [] => "",
    };
    let id = if candidate.len() == 12 { &candidate[..8] } else { candidate };
    if id.len() != 8 || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
        bail!("Could not find a Lichess game id in '{input}'.");
    }
    Ok(id.to_string())
}

const EXPORT_PARAMS: &str = "pgnInJson=true&opening=true&clocks=true&evals=false&literate=false";

pub async fn import_game(db: &Db, client: &reqwest::Client, input: &str, cancel: &AtomicBool) -> Result<(Inserted, i64)> {
    let id = parse_game_id(input)?;
    let url = format!("https://lichess.org/game/export/{id}?{EXPORT_PARAMS}");
    let response = get(client, &url, "application/json", Duration::from_secs(60), cancel, |_| {})
        .await
        .map_err(|e| if e.is::<NotFound>() { anyhow!("Lichess has no game '{id}'.") } else { e })?;
    let game: Game = response.json().await?;
    match convert(&game, None) {
        Converted::Skip("in progress") => bail!("That game is still being played. OUTPOST only imports finished games."),
        Converted::Skip(reason) => bail!("Could not import this game ({reason})."),
        Converted::Game(new) => {
            let inserted = games::insert(db, &new)?;
            let id = match inserted {
                Inserted::New(id) | Inserted::Duplicate(id) => id,
            };
            Ok((inserted, id))
        }
    }
}

pub fn export_url(username: &str, opts: &Options) -> String {
    let mut url = format!("https://lichess.org/api/games/user/{username}?{EXPORT_PARAMS}&finished=true&ongoing=false&sort=dateDesc");
    if let Some(max) = opts.max_games {
        url.push_str(&format!("&max={}", max.clamp(1, 10_000)));
    }
    if let Some(since) = opts.since {
        url.push_str(&format!("&since={}", since * 1000));
    }
    if let Some(until) = opts.until {
        url.push_str(&format!("&until={}", until * 1000));
    }
    if opts.rated_only {
        url.push_str("&rated=true");
    }
    let perfs: Vec<&str> = opts
        .perf_types
        .iter()
        .map(String::as_str)
        .filter(|p| ["ultraBullet", "bullet", "blitz", "rapid", "classical", "correspondence"].contains(p))
        .collect();
    if !perfs.is_empty() {
        url.push_str(&format!("&perfType={}", perfs.join(",")));
    }
    url
}

pub async fn import(
    db: &Db,
    client: &reqwest::Client,
    opts: Options,
    cancel: &AtomicBool,
    mut progress: impl FnMut(&ImportProgress),
) -> Result<ImportProgress> {
    let username = valid_username(&opts.username)?;
    let key = username.to_lowercase();
    let mut opts = opts;
    if opts.incremental {
        let newest: Option<i64> = db
            .conn()
            .query_row("SELECT newest_game_at FROM accounts WHERE platform='lichess' AND username=?1", [&key], |r| r.get(0))
            .optional()?
            .flatten();
        if let Some(newest) = newest {
            opts.since = Some(opts.since.map_or(newest + 1, |s| s.max(newest + 1)));
        }
    }
    let mut state = ImportProgress { message: format!("Requesting {username}'s games from Lichess…"), ..Default::default() };
    progress(&state);
    let url = export_url(&username, &opts);
    let response = get(client, &url, "application/x-ndjson", Duration::from_secs(61), cancel, |m| state.message = m.to_string())
        .await
        .map_err(|e| if e.is::<NotFound>() { anyhow!("Lichess has no player named '{username}'.") } else { e })?;
    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    let mut newest_seen: Option<i64> = None;
    state.message = "Receiving games…".into();
    progress(&state);
    let mut handle_line = |line: &[u8], state: &mut ImportProgress| -> Result<()> {
        let line = std::str::from_utf8(line).unwrap_or("").trim();
        if line.is_empty() {
            return Ok(());
        }
        state.scanned += 1;
        let Ok(game) = serde_json::from_str::<Game>(line) else {
            state.skipped += 1;
            return Ok(());
        };
        match convert(&game, Some(&key)) {
            Converted::Skip(_) => state.skipped += 1,
            Converted::Game(new) => {
                newest_seen = newest_seen.max(new.played_at);
                match games::insert(db, &new)? {
                    Inserted::New(_) => state.imported += 1,
                    Inserted::Duplicate(_) => state.duplicates += 1,
                }
            }
        }
        Ok(())
    };
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::Relaxed) {
            state.message = format!("Stopped. Imported {} game(s).", state.imported);
            break;
        }
        buffer.extend_from_slice(&chunk?);
        while let Some(pos) = buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = buffer.drain(..=pos).collect();
            handle_line(&line, &mut state)?;
        }
        if buffer.len() > 4 * 1024 * 1024 {
            bail!("Unexpectedly long line in the Lichess response.");
        }
        if state.scanned % 20 == 0 {
            progress(&state);
        }
    }
    let rest = std::mem::take(&mut buffer);
    handle_line(&rest, &mut state)?;
    db.conn().execute(
        "INSERT INTO accounts(platform,username,last_sync,newest_game_at) VALUES('lichess',?1,?2,?3)
         ON CONFLICT(platform,username) DO UPDATE SET last_sync=excluded.last_sync, newest_game_at=MAX(COALESCE(accounts.newest_game_at,0), COALESCE(excluded.newest_game_at,0))",
        params![key, now(), newest_seen],
    )?;
    state.done = true;
    if !cancel.load(Ordering::Relaxed) {
        state.message = format!("Imported {} new game(s) from Lichess.", state.imported);
    }
    progress(&state);
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NDJSON: &str = include_str!("../../tests/fixtures/lichess-games.ndjson");

    fn games() -> Vec<Game> {
        NDJSON.lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    #[test]
    fn converts_finished_standard_games_only() {
        let all = games();
        let Converted::Game(g) = convert(&all[0], Some("LiTesterOne")) else { panic!() };
        assert_eq!(g.source_id.as_deref(), Some("AbCdEf12"));
        assert_eq!(g.url.as_deref(), Some("https://lichess.org/AbCdEf12"));
        assert_eq!((g.white.as_str(), g.black.as_str()), ("LiTesterOne", "LiTesterTwo"));
        assert_eq!(g.white_elo, Some(1750));
        assert_eq!(g.result, "0-1");
        assert_eq!(g.speed.as_deref(), Some("blitz"));
        assert_eq!(g.eco.as_deref(), Some("C38"));
        assert_eq!(g.played_at, Some(1794070438));
        assert_eq!(g.ply_count, 10);
        assert!(matches!(convert(&all[1], None), Converted::Skip("in progress")));
        assert!(matches!(convert(&all[2], None), Converted::Skip("variant")));
        assert!(matches!(convert(&all[3], None), Converted::Game(_)));
    }

    #[test]
    fn game_ids_from_urls() {
        assert_eq!(parse_game_id("AbCdEf12").unwrap(), "AbCdEf12");
        assert_eq!(parse_game_id("https://lichess.org/AbCdEf12").unwrap(), "AbCdEf12");
        assert_eq!(parse_game_id("https://lichess.org/AbCdEf12/black#33").unwrap(), "AbCdEf12");
        assert_eq!(parse_game_id("lichess.org/AbCdEf12wxyz").unwrap(), "AbCdEf12");
        assert_eq!(parse_game_id("https://lichess.org/AbCdEf12wxyz").unwrap(), "AbCdEf12");
        assert_eq!(parse_game_id("https://lichess.org/game/export/AbCdEf12?x=1").unwrap(), "AbCdEf12");
        assert!(parse_game_id("https://evil.example/AbCdEf12").is_err());
        assert!(parse_game_id("../../etc/pa").is_err());
    }

    #[test]
    fn export_url_options() {
        let opts = Options { max_games: Some(50), perf_types: vec!["blitz".into(), "bogus".into()], since: Some(10), rated_only: true, ..Default::default() };
        let url = export_url("someone", &opts);
        assert!(url.starts_with("https://lichess.org/api/games/user/someone?"));
        assert!(url.contains("&max=50") && url.contains("&perfType=blitz") && !url.contains("bogus"));
        assert!(url.contains("&since=10000") && url.contains("&rated=true") && url.contains("ongoing=false"));
    }
}
