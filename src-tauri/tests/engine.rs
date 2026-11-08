//! Engine process management against a scripted fake engine (always) and the real
//! Stockfish binary (when OUTPOST_TEST_STOCKFISH points at one).

use outpost_lib::engine::actor::{SearchEvent, SearchLimit, SearchRequest};
use outpost_lib::engine::uci::Score;
use outpost_lib::engine::{self, process, EngineDefaults, EngineRecord};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;

fn fake_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-uci.sh")
}

fn record(path: PathBuf) -> EngineRecord {
    EngineRecord {
        id: "test".into(),
        name: "test".into(),
        path: path.to_string_lossy().into(),
        author: None,
        options: BTreeMap::new(),
        available_options: vec![],
        managed: false,
        builtin: false,
        created_at: 0,
    }
}

fn request(id: u64, moves: &[&str], limit: SearchLimit) -> SearchRequest {
    SearchRequest { search_id: id, fen: None, moves: moves.iter().map(|m| m.to_string()).collect(), multipv: 1, limit }
}

async fn next(rx: &mut mpsc::UnboundedReceiver<SearchEvent>) -> SearchEvent {
    tokio::time::timeout(Duration::from_secs(10), rx.recv()).await.expect("timed out").expect("channel closed")
}

#[tokio::test]
async fn probe_reads_identity_and_options() {
    let identity = process::probe(&fake_path()).await.unwrap();
    assert_eq!(identity.name, "FakeFish 1");
    assert_eq!(identity.author.as_deref(), Some("OUTPOST tests"));
    assert_eq!(identity.options.len(), 4);
    assert_eq!(identity.options[3].vars, ["normal", "wild"]);
}

#[tokio::test]
async fn probe_rejects_non_engines() {
    assert!(process::probe(std::path::Path::new("/bin/true")).await.is_err());
    assert!(process::probe(std::path::Path::new("/definitely/not/here")).await.is_err());
}

#[tokio::test]
async fn finite_search_reports_san_and_done() {
    let handle = engine::launch(&record(fake_path()), &EngineDefaults::default()).await.unwrap();
    let snap = handle.search_once(request(1, &[], SearchLimit { depth: Some(2), ..Default::default() })).await.unwrap();
    assert_eq!(snap.search_id, 1);
    assert_eq!(snap.lines[0].san, ["e4"]);
    assert_eq!(snap.lines[0].score, Score::Mate(3));
    // Black to move: engine's "mate 3" for Black is negative from White's side.
    let snap = handle.search_once(request(2, &["e2e4"], SearchLimit { depth: Some(2), ..Default::default() })).await.unwrap();
    assert_eq!(snap.lines[0].san, ["e5"]);
    assert_eq!(snap.lines[0].score, Score::Mate(-3));
    handle.quit().await;
}

#[tokio::test]
async fn superseded_search_never_leaks_results() {
    let handle = engine::launch(&record(fake_path()), &EngineDefaults::default()).await.unwrap();
    let (tx_a, mut rx_a) = mpsc::unbounded_channel();
    handle.search(request(10, &[], SearchLimit::default()), tx_a).unwrap();
    match next(&mut rx_a).await {
        SearchEvent::Update { snapshot } => assert_eq!(snapshot.search_id, 10),
        other => panic!("expected update, got {other:?}"),
    }
    // A new position arrives while the infinite search runs.
    let (tx_b, mut rx_b) = mpsc::unbounded_channel();
    handle.search(request(11, &["e2e4"], SearchLimit { depth: Some(2), ..Default::default() }), tx_b).unwrap();
    loop {
        match next(&mut rx_b).await {
            SearchEvent::Update { snapshot } => assert_eq!(snapshot.search_id, 11),
            SearchEvent::Done { snapshot, best_move } => {
                assert_eq!(snapshot.search_id, 11);
                assert_eq!(best_move.as_deref(), Some("e7e5"));
                assert!(snapshot.lines.iter().all(|l| l.san == ["e5"]));
                break;
            }
            SearchEvent::Error { message, .. } => panic!("{message}"),
        }
    }
    // The superseded search got nothing further: not even its bestmove.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(rx_a.try_recv().is_err());
    handle.quit().await;
}

#[tokio::test]
async fn stop_finishes_infinite_search() {
    let handle = engine::launch(&record(fake_path()), &EngineDefaults::default()).await.unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    handle.search(request(5, &[], SearchLimit::default()), tx).unwrap();
    assert!(matches!(next(&mut rx).await, SearchEvent::Update { .. }));
    handle.stop();
    match next(&mut rx).await {
        SearchEvent::Done { snapshot, best_move } => {
            assert_eq!(snapshot.search_id, 5);
            assert_eq!(best_move.as_deref(), Some("e2e4"));
            assert_eq!(snapshot.lines[0].score, Score::Cp(20), "bound lines are ignored");
        }
        other => panic!("expected done, got {other:?}"),
    }
    handle.quit().await;
}

#[tokio::test]
async fn illegal_requests_never_reach_the_engine() {
    let handle = engine::launch(&record(fake_path()), &EngineDefaults::default()).await.unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    handle.search(request(3, &["e2e5"], SearchLimit::default()), tx).unwrap();
    assert!(matches!(next(&mut rx).await, SearchEvent::Error { search_id: 3, .. }));
    let bad_fen = SearchRequest { fen: Some("8/8/8/8/8/8/8/8 w - - 0 1".into()), ..request(4, &[], SearchLimit::default()) };
    let (tx, mut rx) = mpsc::unbounded_channel();
    handle.search(bad_fen, tx).unwrap();
    assert!(matches!(next(&mut rx).await, SearchEvent::Error { search_id: 4, .. }));
    // Still alive afterwards.
    let snap = handle.search_once(request(6, &[], SearchLimit { depth: Some(1), ..Default::default() })).await.unwrap();
    assert_eq!(snap.search_id, 6);
    handle.quit().await;
}

#[tokio::test]
async fn crash_is_reported_and_handle_dies() {
    let handle = engine::launch(&record(fake_path()), &EngineDefaults::default()).await.unwrap();
    let err = handle.search_once(request(7, &[], SearchLimit { nodes: Some(13), ..Default::default() })).await.unwrap_err();
    assert!(err.to_string().contains("stopped unexpectedly"), "{err}");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!handle.is_alive());
}

#[tokio::test]
async fn configure_changes_options_while_idle() {
    let handle = engine::launch(&record(fake_path()), &EngineDefaults::default()).await.unwrap();
    handle.configure(vec![("Hash".into(), Some("32".into()))]).await.unwrap();
    let snap = handle.search_once(request(8, &[], SearchLimit { depth: Some(1), ..Default::default() })).await.unwrap();
    assert_eq!(snap.search_id, 8);
    handle.quit().await;
}

#[tokio::test]
async fn real_stockfish_multipv_and_mate() {
    let Some(path) = std::env::var_os("OUTPOST_TEST_STOCKFISH").map(PathBuf::from) else {
        eprintln!("skipping: set OUTPOST_TEST_STOCKFISH to a Stockfish binary");
        return;
    };
    let mut rec = record(path);
    rec.options.insert("Threads".into(), "2".into());
    let handle = engine::launch(&rec, &EngineDefaults { threads: 2, hash_mb: 64 }).await.unwrap();
    assert!(handle.identity.name.starts_with("Stockfish"));
    let mut req = request(1, &["e2e4", "e7e5"], SearchLimit { depth: Some(12), ..Default::default() });
    req.multipv = 3;
    let snap = handle.search_once(req).await.unwrap();
    assert_eq!(snap.lines.len(), 3);
    assert!(snap.depth >= 12);
    assert!(snap.lines.iter().all(|l| !l.san.is_empty() && l.wdl.is_some()));
    // Scholar's mate pattern: White mates in 1 with Qxf7#.
    let fen = "r1bqkb1r/pppp1ppp/2n2n2/4p2Q/2B1P3/8/PPPP1PPP/RNB1K1NR w KQkq - 4 4";
    let req = SearchRequest { fen: Some(fen.into()), ..request(2, &[], SearchLimit { depth: Some(8), ..Default::default() }) };
    let snap = handle.search_once(req).await.unwrap();
    assert_eq!(snap.lines[0].score, Score::Mate(1));
    assert_eq!(snap.lines[0].san[0], "Qxf7#");
    // Rapid re-requests: only the last one answers.
    let (tx, mut rx) = mpsc::unbounded_channel();
    for id in 100..110 {
        handle.search(request(id, &["d2d4"], SearchLimit::default()), tx.clone()).unwrap();
    }
    handle.search(request(200, &["g1f3"], SearchLimit { depth: Some(10), ..Default::default() }), tx).unwrap();
    loop {
        match next(&mut rx).await {
            SearchEvent::Update { snapshot } => assert!(snapshot.search_id == 200 || snapshot.search_id == 100, "stale id {}", snapshot.search_id),
            SearchEvent::Done { snapshot, .. } => { assert_eq!(snapshot.search_id, 200); break; }
            SearchEvent::Error { message, .. } => panic!("{message}"),
        }
    }
    handle.quit().await;
}
