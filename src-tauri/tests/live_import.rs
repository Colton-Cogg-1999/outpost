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
