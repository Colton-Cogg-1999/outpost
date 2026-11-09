//! Read-only, bounded history pages. Nothing is persisted until a row is selected.
use super::{chesscom, get, lichess, valid_username};
use crate::games::NewGame;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::{sync::atomic::{AtomicBool, Ordering}, time::Duration};
use futures_util::StreamExt;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub platform: String,
    pub username: String,
    pub year: i32,
    pub month: u32,
    pub cursor: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub games: Vec<NewGame>,
    pub next_cursor: Option<i64>,
}

async fn bounded_body(response: reqwest::Response, cancel: &AtomicBool) -> Result<Vec<u8>> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::Relaxed) { bail!("Cancelled."); }
        let chunk = chunk?;
        if body.len() + chunk.len() > 16 * 1024 * 1024 { bail!("History response is too large. Try another month."); }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub async fn browse(client: &reqwest::Client, req: Request, cancel: &AtomicBool) -> Result<Page> {
    let username = valid_username(&req.username)?.to_lowercase();
    if !(2000..=2200).contains(&req.year) || !(1..=12).contains(&req.month) { bail!("Choose a valid month."); }
    match req.platform.as_str() {
        "lichess" => {
            let end = chesscom::month_end_unix(req.year, req.month) * 1000 - 1;
            let (y, m) = if req.month == 1 { (req.year - 1, 12) } else { (req.year, req.month - 1) };
            let start = chesscom::month_end_unix(y, m) * 1000;
            let until = req.cursor.unwrap_or(end).min(end);
            if until < start { return Ok(Page { games: vec![], next_cursor: None }); }
            let url = format!("https://lichess.org/api/games/user/{username}?pgnInJson=true&opening=true&evals=false&clocks=false&finished=true&ongoing=false&sort=dateDesc&max=20&since={start}&until={until}");
            let response = get(client, &url, "application/x-ndjson", Duration::from_secs(61), cancel, |_| {}).await?;
            let bytes = bounded_body(response, cancel).await?;
            lichess_page(&bytes, &username, start)
        }
        "chesscom" => {
            let url = format!("https://api.chess.com/pub/player/{username}/games/{}/{:02}", req.year, req.month);
            let response = get(client, &url, "application/json", Duration::from_secs(60), cancel, |_| {}).await?;
            let bytes = bounded_body(response, cancel).await?;
            chesscom_page(&bytes, &username, req.cursor)
        }
        _ => bail!("Unknown game service."),
    }
}

fn lichess_page(bytes: &[u8], username: &str, start: i64) -> Result<Page> {
            let mut games = Vec::new();
            let mut count = 0;
            let mut oldest = None;
            for line in bytes.split(|b| *b == b'\n').filter(|l| !l.iter().all(u8::is_ascii_whitespace)) {
                let game: lichess::Game = serde_json::from_slice(line)?;
                count += 1;
                if let Some(created) = game.created_at { oldest = Some(oldest.map_or(created, |n: i64| n.min(created))); }
                if let lichess::Converted::Game(game) = lichess::convert(&game, Some(username)) { games.push(*game); }
            }
            Ok(Page { games, next_cursor: if count == 20 { oldest.map(|n| n - 1).filter(|n| *n >= start) } else { None } })
}

fn chesscom_page(bytes: &[u8], username: &str, cursor: Option<i64>) -> Result<Page> {
            let mut month: chesscom::Month = serde_json::from_slice(&bytes)?;
            month.games.sort_by_key(|g| std::cmp::Reverse(g.end_time.unwrap_or(0)));
            let offset = cursor.unwrap_or(0).max(0) as usize;
            let total = month.games.len();
            let games = month.games.into_iter().skip(offset).take(20).filter_map(|g| match chesscom::convert(&g, username) {
                chesscom::Converted::Game(game) => Some(*game), _ => None,
            }).collect();
            Ok(Page { games, next_cursor: (offset.saturating_add(20) < total).then_some(offset.saturating_add(20) as i64) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lichess_history_skips_unfinished_and_variants() {
        let page = lichess_page(include_bytes!("../../tests/fixtures/lichess-games.ndjson"), "tester", 0).unwrap();
        assert_eq!(page.games.len(), 2);
        assert!(page.games.iter().all(|g| g.account.as_deref() == Some("tester")));
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn lichess_cursor_uses_creation_time_in_milliseconds() {
        let fixture = include_str!("../../tests/fixtures/lichess-games.ndjson").lines().next().unwrap();
        let game: lichess::Game = serde_json::from_str(fixture).unwrap();
        let body = std::iter::repeat_n(fixture, 20).collect::<Vec<_>>().join("\n");
        let page = lichess_page(body.as_bytes(), "tester", 0).unwrap();
        assert_eq!(page.games.len(), 20);
        assert_eq!(page.next_cursor, game.created_at.map(|n| n - 1));
    }

    #[test]
    fn chesscom_history_is_bounded_and_exhausts() {
        let fixture: serde_json::Value = serde_json::from_slice(include_bytes!("../../tests/fixtures/chesscom-month.json")).unwrap();
        let body = serde_json::to_vec(&serde_json::json!({"games": vec![fixture["games"][0].clone(); 25]})).unwrap();
        let first = chesscom_page(&body, "tester", None).unwrap();
        assert_eq!(first.games.len(), 20);
        assert_eq!(first.next_cursor, Some(20));
        let second = chesscom_page(&body, "tester", first.next_cursor).unwrap();
        assert_eq!(second.games.len(), 5);
        assert!(second.next_cursor.is_none());
    }
}
