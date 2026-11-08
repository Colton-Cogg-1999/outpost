//! Whole-game review with the real engine (set OUTPOST_TEST_STOCKFISH).

use outpost_lib::analysis::classify::Classification;
use outpost_lib::analysis::review::{self, ReviewEvent, ReviewSettings};
use outpost_lib::db::Db;
use outpost_lib::engine::{self, EngineDefaults, EngineRecord};
use std::sync::atomic::AtomicBool;

#[tokio::test]
async fn reviews_a_game_classifies_blunders_and_caches() {
    let Some(path) = std::env::var_os("OUTPOST_TEST_STOCKFISH") else {
        eprintln!("skipping: set OUTPOST_TEST_STOCKFISH");
        return;
    };
    let rec = EngineRecord {
        id: "sf".into(), name: "sf".into(), path: path.to_string_lossy().into(), author: None,
        options: Default::default(), available_options: vec![], managed: false, builtin: false, created_at: 0,
    };
    let handle = engine::launch(&rec, &EngineDefaults { threads: 2, hash_mb: 64 }).await.unwrap();
    let db = Db::in_memory().unwrap();
    // 3...Nf6?? walks into Scholar's mate; 4.Qxf7# ends it.
    let moves: Vec<String> = ["e2e4", "e7e5", "f1c4", "b8c6", "d1h5", "g8f6", "h5f7"].map(String::from).to_vec();
    let settings = ReviewSettings { depth: Some(12), ..Default::default() };
    let mut progress = 0;
    let report = review::run(&db, &handle, None, &moves, &settings, &AtomicBool::new(false), |e| {
        if matches!(e, ReviewEvent::Progress { .. }) { progress += 1; }
    }).await.unwrap();
    assert_eq!(progress, 8);
    assert_eq!(report.moves.len(), 7);
    let nf6 = &report.moves[5];
    assert_eq!(nf6.san, "Nf6");
    assert_eq!(nf6.verdict.classification, Classification::Blunder, "{:?}", nf6.verdict);
    assert!(nf6.verdict.reasons.iter().any(|r| r.contains("mate")), "{:?}", nf6.verdict.reasons);
    let mate = &report.moves[6];
    assert_eq!(mate.san, "Qxf7#");
    assert!(matches!(mate.verdict.classification, Classification::Best | Classification::Great));
    assert_eq!(report.positions.last().unwrap().terminal.as_deref(), Some("checkmate"));
    assert!(report.turning_points.iter().any(|t| t.ply == 6));

    // Second run reuses the cache for every non-terminal position.
    let again = review::run(&db, &handle, None, &moves, &settings, &AtomicBool::new(false), |_| {}).await.unwrap();
    assert_eq!(again.positions.iter().filter(|p| p.cached).count(), 7);

    // Cancellation stops the job.
    let cancel = AtomicBool::new(true);
    assert!(review::run(&db, &handle, None, &moves, &settings, &cancel, |_| {}).await.is_err());
    handle.quit().await;
}
