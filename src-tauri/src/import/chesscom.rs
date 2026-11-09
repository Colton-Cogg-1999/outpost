//! Chess.com Published-Data API (https://www.chess.com/news/view/published-data-api).
//! Public, read-only: monthly archives of a player's *completed* games.
//! OUTPOST is not affiliated with or endorsed by Chess.com.

use super::{get, valid_username, ImportProgress, NotFound};
use crate::chess::pgn;
use crate::db::Db;
use crate::games::{self, Inserted, NewGame};
use crate::paths::now;
use anyhow::{anyhow, Result};
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

#[derive(Deserialize)]
struct Archives {
    archives: Vec<String>,
}

#[derive(Deserialize)]
pub struct Month {
    pub games: Vec<Game>,
}

#[derive(Deserialize, Clone)]
pub struct Player {
    pub username: String,
    pub rating: Option<i64>,
    pub result: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct Game {
    pub url: String,
    pub pgn: Option<String>,
    pub time_control: Option<String>,
    pub end_time: Option<i64>,
    pub rated: Option<bool>,
    pub time_class: Option<String>,
    pub rules: Option<String>,
    pub uuid: Option<String>,
    pub white: Player,
    pub black: Player,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Options {
    pub username: String,
    pub max_games: Option<usize>,
    /// bullet | blitz | rapid | daily (empty = all)
    pub time_classes: Vec<String>,
    /// Unix seconds.
    pub since: Option<i64>,
    pub until: Option<i64>,
    pub rated_only: bool,
    /// Only fetch games newer than the last sync of this account.
    pub incremental: bool,
}

pub enum Converted {
    Game(Box<NewGame>),
    Skip(&'static str),
}

pub fn convert(game: &Game, account: &str) -> Converted {
    if game.rules.as_deref().unwrap_or("chess") != "chess" {
        return Converted::Skip("variant");
    }
    let Some(text) = game.pgn.as_deref() else { return Converted::Skip("no PGN") };
    let Some(parsed) = pgn::parse_one(text) else { return Converted::Skip("unreadable PGN") };
    if parsed.error.is_some() || parsed.moves_uci.is_empty() {
        return Converted::Skip("invalid PGN");
    }
    let mut new = NewGame::from_parsed(&parsed, "chesscom");
    new.source_id = Some(game.uuid.clone().unwrap_or_else(|| game.url.clone()));
    new.account = Some(account.to_lowercase());
    new.url = Some(game.url.clone()).filter(|u| u.starts_with("https://www.chess.com/"));
    new.white = game.white.username.clone();
    new.black = game.black.username.clone();
    new.white_elo = game.white.rating.or(new.white_elo);
    new.black_elo = game.black.rating.or(new.black_elo);
    new.played_at = game.end_time;
    new.time_control = game.time_control.clone().or(new.time_control);
    new.speed = match game.time_class.as_deref() {
        Some("daily") => Some("correspondence".into()),
        Some(c) => Some(c.to_string()),
        None => new.speed,
    };
    Converted::Game(Box::new(new))
}

/// "https://api.chess.com/pub/player/x/games/2024/05" -> (2024, 5)
fn archive_month(url: &str) -> Option<(i32, u32)> {
    let mut parts = url.rsplit('/');
    let month = parts.next()?.parse().ok()?;
    let year = parts.next()?.parse().ok()?;
    Some((year, month))
}

pub(super) fn month_end_unix(year: i32, month: u32) -> i64 {
    // Days from civil (Howard Hinnant) for the first day of the following month.
    let (y, m) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146097 + doe - 719468) * 86400
}

pub fn wanted(game: &Game, opts: &Options) -> bool {
    let end = game.end_time.unwrap_or(0);
    (opts.time_classes.is_empty() || game.time_class.as_ref().is_some_and(|c| opts.time_classes.contains(c)))
        && opts.since.is_none_or(|s| end >= s)
        && opts.until.is_none_or(|u| end <= u)
        && (!opts.rated_only || game.rated == Some(true))
}

pub async fn import(
    db: &Db,
    client: &reqwest::Client,
    opts: Options,
    cancel: &AtomicBool,
    mut progress: impl FnMut(&ImportProgress),
) -> Result<ImportProgress> {
    let username = valid_username(&opts.username)?.to_lowercase();
    let mut opts = opts;
    if opts.incremental {
        let newest: Option<i64> = db
            .conn()
            .query_row("SELECT newest_game_at FROM accounts WHERE platform='chesscom' AND username=?1", [&username], |r| r.get(0))
            .optional()?
            .flatten();
        if let Some(newest) = newest {
            opts.since = Some(opts.since.map_or(newest, |s| s.max(newest)));
        }
    }
    let max = opts.max_games.unwrap_or(usize::MAX).max(1);
    let mut state = ImportProgress { message: format!("Looking up {username} on Chess.com…"), ..Default::default() };
    progress(&state);
    let base = format!("https://api.chess.com/pub/player/{username}/games/");
    let response = get(client, &format!("{base}archives"), "application/json", Duration::from_secs(60), cancel, |m| {
        state.message = m.to_string();
    })
    .await
    .map_err(|e| if e.is::<NotFound>() { anyhow!("Chess.com has no player named '{username}'.") } else { e })?;
    let archives: Archives = response.json().await?;
    let mut months: Vec<String> = archives.archives.into_iter().filter(|u| u.starts_with(&base)).collect();
    months.reverse();
    let mut newest_seen: Option<i64> = None;
    'months: for url in months {
        if let (Some(since), Some((y, m))) = (opts.since, archive_month(&url)) {
            if month_end_unix(y, m) < since {
                break;
            }
        }
        state.message = match archive_month(&url) {
            Some((y, m)) => format!("Reading {y}-{m:02}…"),
            None => "Reading archive…".into(),
        };
        progress(&state);
        // Serial and unhurried, as the API documentation asks.
        super::sleep_cancellable(Duration::from_millis(250), cancel).await?;
        let response = match get(client, &url, "application/json", Duration::from_secs(60), cancel, |_| {}).await {
            Ok(r) => r,
            Err(e) if e.is::<NotFound>() => continue,
            Err(e) => return Err(e),
        };
        let mut month: Month = response.json().await?;
        month.games.sort_by_key(|g| std::cmp::Reverse(g.end_time.unwrap_or(0)));
        for game in &month.games {
            state.scanned += 1;
            if !wanted(game, &opts) {
                state.skipped += 1;
                continue;
            }
            match convert(game, &username) {
                Converted::Skip(_) => state.skipped += 1,
                Converted::Game(new) => {
                    newest_seen = newest_seen.max(new.played_at);
                    match games::insert(db, &new)? {
                        Inserted::New(_) => state.imported += 1,
                        Inserted::Duplicate(_) => state.duplicates += 1,
                    }
                }
            }
            if state.imported >= max {
                break 'months;
            }
        }
        progress(&state);
    }
    db.conn().execute(
        "INSERT INTO accounts(platform,username,last_sync,newest_game_at) VALUES('chesscom',?1,?2,?3)
         ON CONFLICT(platform,username) DO UPDATE SET last_sync=excluded.last_sync, newest_game_at=MAX(COALESCE(accounts.newest_game_at,0), COALESCE(excluded.newest_game_at,0))",
        params![username, now(), newest_seen],
    )?;
    state.done = true;
    state.message = format!("Imported {} new game(s) from Chess.com.", state.imported);
    progress(&state);
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONTH: &str = include_str!("../../tests/fixtures/chesscom-month.json");

    #[test]
    fn converts_fixture_games() {
        let month: Month = serde_json::from_str(MONTH).unwrap();
        assert_eq!(month.games.len(), 3);
        let Converted::Game(g) = convert(&month.games[0], "TestPlayerA") else { panic!("first game should convert") };
        assert_eq!(g.source, "chesscom");
        assert_eq!(g.white, "TestPlayerA");
        assert_eq!(g.white_elo, Some(1493));
        assert_eq!(g.speed.as_deref(), Some("blitz"));
        assert_eq!(g.played_at, Some(1793540394));
        assert_eq!(g.account.as_deref(), Some("testplayera"));
        assert_eq!(g.result, "0-1");
        assert!(g.pgn.contains("1. e4 { [%clk 0:02:59.9] } 1... e5"), "clock annotations are kept as comments: {}", g.pgn);
        assert!(matches!(convert(&month.games[1], "x"), Converted::Skip("variant")));
        assert!(matches!(convert(&month.games[2], "x"), Converted::Game(_)));
    }

    #[test]
    fn filters_by_class_date_and_rating() {
        let month: Month = serde_json::from_str(MONTH).unwrap();
        let opts = Options { time_classes: vec!["daily".into()], ..Default::default() };
        assert!(!wanted(&month.games[0], &opts));
        assert!(wanted(&month.games[2], &opts));
        let opts = Options { since: Some(1793540395), ..Default::default() };
        assert!(!wanted(&month.games[0], &opts));
        let opts = Options { rated_only: true, ..Default::default() };
        assert!(!wanted(&month.games[2], &opts));
    }

    #[test]
    fn import_into_db_deduplicates() {
        let db = Db::in_memory().unwrap();
        let month: Month = serde_json::from_str(MONTH).unwrap();
        for _ in 0..2 {
            for g in &month.games {
                if let Converted::Game(n) = convert(g, "a") {
                    games::insert(&db, &n).unwrap();
                }
            }
        }
        assert_eq!(games::list(&db, &Default::default()).unwrap().total, 2);
    }

    #[test]
    fn month_arithmetic() {
        assert_eq!(archive_month("https://api.chess.com/pub/player/x/games/2024/05"), Some((2024, 5)));
        assert_eq!(month_end_unix(2024, 5), 1717200000); // 2024-06-01T00:00:00Z
        assert_eq!(month_end_unix(2023, 12), 1704067200); // 2024-01-01
    }
}
