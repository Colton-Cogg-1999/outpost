//! Live API checks. Opt-in: OUTPOST_LIVE_USER=<name> cargo test --test live_import -- --nocapture
use outpost_lib::{db::Db, games, import};
use std::sync::atomic::AtomicBool;

fn user() -> Option<String> {
    std::env::var("OUTPOST_LIVE_USER").ok().filter(|s| !s.is_empty())
}

#[tokio::test]
async fn chesscom_live() {
    let Some(u) = user() else { return };
    let db = Db::in_memory().unwrap();
    let client = import::client().unwrap();
    let opts = import::chesscom::Options { username: u, max_games: Some(30), ..Default::default() };
    let r = import::chesscom::import(&db, &client, opts, &AtomicBool::new(false), |p| eprintln!("chesscom: {} scanned={} imported={} skipped={}", p.message, p.scanned, p.imported, p.skipped)).await;
    eprintln!("chesscom result: {:?}", r.as_ref().map(|s| (s.imported, s.skipped)).map_err(|e| format!("{e:#}")));
    eprintln!("chesscom listed: {}", games::list(&db, &Default::default()).unwrap().total);
    r.unwrap();
}

#[tokio::test]
async fn lichess_live() {
    let Some(u) = user() else { return };
    let db = Db::in_memory().unwrap();
    let client = import::client().unwrap();
    let opts = import::lichess::Options { username: u, max_games: Some(30), ..Default::default() };
    let r = import::lichess::import(&db, &client, opts, &AtomicBool::new(false), |p| eprintln!("lichess: {} scanned={} imported={}", p.message, p.scanned, p.imported)).await;
    eprintln!("lichess result: {:?}", r.as_ref().map(|s| (s.imported, s.skipped)).map_err(|e| format!("{e:#}")));
    eprintln!("lichess listed: {}", games::list(&db, &Default::default()).unwrap().total);
    r.unwrap();
}

/// Read-only history smoke test; never writes games to a database.
#[tokio::test]
async fn history_live() {
    if std::env::var("OUTPOST_LIVE_HISTORY").is_err() { return; }
    let client = import::client().unwrap();
    for (platform, username) in [("lichess", "thibault"), ("chesscom", "erik")] {
        let page = import::history::browse(&client, import::history::Request {
            platform: platform.into(), username: username.into(), year: 2025, month: 1, cursor: None,
        }, &AtomicBool::new(false)).await.unwrap();
        assert!(!page.games.is_empty(), "{platform}: expected public fixture account history");
        assert!(page.games.len() <= 20);
        eprintln!("{platform}: {} selectable games, older page: {}", page.games.len(), page.next_cursor.is_some());
        if let Some(cursor) = page.next_cursor {
            let older = import::history::browse(&client, import::history::Request {
                platform: platform.into(), username: username.into(), year: 2025, month: 1, cursor: Some(cursor),
            }, &AtomicBool::new(false)).await.unwrap();
            assert!(older.games.iter().all(|g| page.games.iter().all(|first| first.source_id != g.source_id)));
        }
    }
}
