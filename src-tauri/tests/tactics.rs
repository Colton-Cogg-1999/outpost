//! Tactic generation end to end with a real engine (set OUTPOST_TEST_STOCKFISH).

use outpost_lib::analysis::review::ReviewSettings;
use outpost_lib::chess::pgn;
use outpost_lib::db::Db;
use outpost_lib::engine::{self, EngineDefaults, EngineRecord};
use outpost_lib::games::{self, Inserted, NewGame};
use outpost_lib::tactics::{self, generator::{Limits, Strictness}};
use std::sync::atomic::AtomicBool;

#[tokio::test]
async fn generates_tactics_from_a_game() {
    let Some(path) = std::env::var_os("OUTPOST_TEST_STOCKFISH") else {
        eprintln!("skipping: set OUTPOST_TEST_STOCKFISH");
        return;
    };
    let rec = EngineRecord {
        id: "sf".into(), name: "sf".into(), path: path.to_string_lossy().into(), author: None,
        options: Default::default(), available_options: vec![], managed: false, builtin: false, created_at: 0,
    };
    let handle = engine::launch(&rec, &EngineDefaults { threads: 2, hash_mb: 16 }).await.unwrap();
    let db = Db::in_memory().unwrap();
    // Légal's mate: 5...Bxd1?? allows 6.Bxf7+ Ke7 7.Nd5#.
    let text = "[White \"me\"]\n[Black \"them\"]\n\n1. e4 e5 2. Nf3 d6 3. Bc4 Bg4 4. Nc3 g6 5. Nxe5 Bxd1 6. Bxf7+ Ke7 7. Nd5# 1-0";
    let mut new = NewGame::from_parsed(&pgn::parse_one(text).unwrap(), "pgn");
    new.account = Some("me".into());
    let Inserted::New(game) = games::insert(&db, &new).unwrap() else { panic!() };
    let settings = ReviewSettings { depth: Some(14), ..Default::default() };
    let limits = Limits {
        pair: outpost_lib::engine::actor::SearchLimit { depth: Some(30), movetime_ms: Some(1500), nodes: Some(1_500_000) },
        ..Default::default()
    };
    let mut stages = vec![];
    let found = tactics::scan_game(&db, &handle, game, &settings, &limits, Strictness::Lichess, &AtomicBool::new(false), |stage, _, _, _| {
        if stages.last().map(String::as_str) != Some(stage) {
            stages.push(stage.to_string());
        }
    })
    .await
    .unwrap();
    handle.quit().await;
    assert!(stages.contains(&"review".to_string()), "{stages:?}");
    let list = tactics::for_game(&db, game).unwrap();
    for t in &list {
        eprintln!("ply {} {} {:?} {:?} owner={:?} played={:?}", t.ply, t.kind, t.moves, t.themes, t.owner, t.played);
    }
    assert_eq!(found, list.len());
    assert!(!list.iter().any(|t| t.ply == 11), "6...Ke7 was forced and on the known solution: not a second tactic");
    let mate = list.iter().find(|t| t.ply == 9).expect("the Légal's mate tactic after 5...Bxd1");
    assert_eq!(mate.moves, ["g4d1", "c4f7", "e8e7", "c3d5"]);
    assert_eq!(mate.solver, "white");
    assert_eq!(mate.owner.as_deref(), Some("you"));
    assert_eq!(mate.played, None, "White found it in the game");
    assert!(mate.themes.iter().any(|t| t == "mateIn2"), "{:?}", mate.themes);
    // The scan also saved a review, and remembers the game was scanned.
    assert!(outpost_lib::analysis::review::load(&db, game).unwrap().is_some());
    assert!(tactics::pending_games(&db, &Default::default()).unwrap().is_empty());
}

/// Opt-in: scan games in a copy of a real library.
/// OUTPOST_TEST_STOCKFISH=… OUTPOST_TACTICS_DB=/copy/of/outpost.db cargo test --test tactics library -- --nocapture
#[tokio::test]
async fn library_scan() {
    let (Some(path), Some(db_path)) = (std::env::var_os("OUTPOST_TEST_STOCKFISH"), std::env::var_os("OUTPOST_TACTICS_DB")) else { return };
    let _ = tracing_subscriber::fmt().with_env_filter(std::env::var("OUTPOST_LOG").unwrap_or_default()).with_test_writer().try_init();
    let rec = EngineRecord {
        id: "sf".into(), name: "sf".into(), path: path.to_string_lossy().into(), author: None,
        options: Default::default(), available_options: vec![], managed: false, builtin: false, created_at: 0,
    };
    let handle = engine::launch(&rec, &EngineDefaults { threads: 4, hash_mb: 16 }).await.unwrap();
    let db = Db::open(std::path::Path::new(&db_path)).unwrap();
    let max = std::env::var("OUTPOST_TACTICS_GAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(5);
    let only: Vec<i64> = std::env::var("OUTPOST_TACTICS_IDS").ok().map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_default();
    let ids = tactics::pending_games(&db, &tactics::ScanSettings { max_games: max, game_ids: only, ..Default::default() }).unwrap();
    let settings = ReviewSettings::default();
    for id in ids {
        let t0 = std::time::Instant::now();
        let n = tactics::scan_game(&db, &handle, id, &settings, &Limits::default(), Strictness::Personal, &AtomicBool::new(false), |_, _, _, _| {}).await.unwrap();
        eprintln!("game {id}: {n} tactic(s) in {:.1}s", t0.elapsed().as_secs_f64());
        for t in tactics::for_game(&db, id).unwrap() {
            eprintln!("  ply {} {} solver={} owner={:?} played={:?} {:?} {:?}", t.ply, t.kind, t.solver, t.owner, t.played, t.moves, t.themes);
        }
    }
    handle.quit().await;
}
