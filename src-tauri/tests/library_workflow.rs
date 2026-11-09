use outpost_lib::{db::Db, games, tactics};

#[test]
fn clearing_history_removes_game_data_and_preserves_settings() {
    let db = Db::in_memory().unwrap();
    db.set_setting("ai", "configured-provider").unwrap();
    let id = games::import_pgn_text(&db, "1. e4 e5 2. Nf3 Nc6 *", "test").unwrap().first_id.unwrap();
    db.conn().execute("INSERT INTO accounts(platform,username) VALUES('lichess','me')", []).unwrap();
    db.conn().execute("INSERT INTO tactic_scans(game_id,version,found,scanned_at) VALUES(?1,1,0,0)", [id]).unwrap();
    db.conn().execute("INSERT INTO analyses(game_id,engine,settings,report,created_at) VALUES(?1,'test','{}','{}',0)", [id]).unwrap();
    assert_eq!(games::clear_history(&db).unwrap(), 1);
    for table in ["games", "analyses", "tactic_scans", "accounts", "tactics"] {
        let count: i64 = db.conn().query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0, "{table}");
    }
    assert_eq!(db.setting("ai").as_deref(), Some("configured-provider"));
    assert_eq!(games::clear_history(&db).unwrap(), 0);
}

#[test]
fn batch_selection_retries_changed_games_but_never_repeats_completed_games() {
    let db = Db::in_memory().unwrap();
    let a = games::import_pgn_text(&db, "1. e4 e5 2. Nf3 Nc6 *", "test").unwrap().first_id.unwrap();
    let b = games::import_pgn_text(&db, "1. d4 d5 2. c4 e6 *", "test").unwrap().first_id.unwrap();
    db.conn().execute("INSERT INTO tactic_scans(game_id,version,found,scanned_at) VALUES(?1,0,0,0)", [a]).unwrap();
    assert_eq!(tactics::pending_games(&db, &Default::default()).unwrap(), [b]);
    let parsed = outpost_lib::chess::pgn::parse_one("1. e4 c5 2. Nf3 Nc6 *").unwrap();
    games::update_pgn(&db, a, &parsed).unwrap();
    let pending = tactics::pending_games(&db, &Default::default()).unwrap();
    assert_eq!(pending.len(), 2);
    assert!(pending.contains(&a));
}
