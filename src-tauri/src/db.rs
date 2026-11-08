//! SQLite persistence. One connection behind a mutex serves interactive commands;
//! long bulk jobs can open their own connection. WAL mode lets both coexist.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// Ordered, append-only list of migrations. Never edit an entry that has shipped;
/// add a new one. `PRAGMA user_version` records how many have been applied.
const MIGRATIONS: &[&str] = &[
    // 1: initial schema
    r#"
    CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);

    CREATE TABLE engines(
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        path TEXT NOT NULL,
        author TEXT,
        options TEXT NOT NULL DEFAULT '{}',
        available_options TEXT NOT NULL DEFAULT '[]',
        managed INTEGER NOT NULL DEFAULT 0,
        created_at INTEGER NOT NULL
    );

    CREATE TABLE games(
        id INTEGER PRIMARY KEY,
        source TEXT NOT NULL,
        source_id TEXT,
        account TEXT,
        url TEXT,
        white TEXT NOT NULL DEFAULT '?',
        black TEXT NOT NULL DEFAULT '?',
        white_elo INTEGER,
        black_elo INTEGER,
        result TEXT NOT NULL DEFAULT '*',
        date TEXT,
        played_at INTEGER,
        time_control TEXT,
        speed TEXT,
        event TEXT,
        site TEXT,
        eco TEXT,
        opening TEXT,
        termination TEXT,
        ply_count INTEGER NOT NULL DEFAULT 0,
        pgn TEXT NOT NULL,
        fingerprint TEXT NOT NULL,
        imported_at INTEGER NOT NULL,
        favorite INTEGER NOT NULL DEFAULT 0
    );
    CREATE UNIQUE INDEX games_source ON games(source, source_id) WHERE source_id IS NOT NULL;
    CREATE UNIQUE INDEX games_fingerprint ON games(fingerprint);
    CREATE INDEX games_played ON games(played_at DESC);
    CREATE INDEX games_white ON games(white COLLATE NOCASE);
    CREATE INDEX games_black ON games(black COLLATE NOCASE);

    CREATE TABLE accounts(
        platform TEXT NOT NULL,
        username TEXT NOT NULL COLLATE NOCASE,
        last_sync INTEGER,
        newest_game_at INTEGER,
        PRIMARY KEY(platform, username)
    );

    CREATE TABLE analyses(
        game_id INTEGER PRIMARY KEY REFERENCES games(id) ON DELETE CASCADE,
        engine TEXT NOT NULL,
        settings TEXT NOT NULL,
        report TEXT NOT NULL,
        created_at INTEGER NOT NULL
    );

    CREATE TABLE evals(
        position TEXT NOT NULL,
        engine TEXT NOT NULL,
        depth INTEGER NOT NULL,
        multipv INTEGER NOT NULL,
        lines TEXT NOT NULL,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY(position, engine)
    ) WITHOUT ROWID;

    CREATE TABLE puzzles(
        id TEXT PRIMARY KEY,
        fen TEXT NOT NULL,
        moves TEXT NOT NULL,
        rating INTEGER NOT NULL,
        rating_dev INTEGER NOT NULL,
        popularity INTEGER NOT NULL,
        plays INTEGER NOT NULL,
        themes TEXT NOT NULL,
        game_url TEXT,
        opening TEXT
    ) WITHOUT ROWID;
    CREATE INDEX puzzles_rating ON puzzles(rating);

    CREATE TABLE puzzle_themes(
        theme TEXT NOT NULL,
        rating INTEGER NOT NULL,
        puzzle_id TEXT NOT NULL,
        PRIMARY KEY(theme, rating, puzzle_id)
    ) WITHOUT ROWID;

    CREATE TABLE puzzle_attempts(
        id INTEGER PRIMARY KEY,
        puzzle_id TEXT NOT NULL,
        solved INTEGER NOT NULL,
        mistakes INTEGER NOT NULL,
        hints INTEGER NOT NULL,
        duration_ms INTEGER NOT NULL,
        rating_before INTEGER,
        rating_after INTEGER,
        attempted_at INTEGER NOT NULL
    );
    CREATE INDEX puzzle_attempts_puzzle ON puzzle_attempts(puzzle_id);

    CREATE TABLE chats(
        id TEXT PRIMARY KEY,
        title TEXT NOT NULL DEFAULT '',
        fen TEXT,
        game_id INTEGER,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE TABLE chat_messages(
        id INTEGER PRIMARY KEY,
        chat_id TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
        role TEXT NOT NULL,
        content TEXT NOT NULL,
        provider TEXT,
        model TEXT,
        fen TEXT,
        created_at INTEGER NOT NULL
    );
    CREATE INDEX chat_messages_chat ON chat_messages(chat_id, id);
    "#,
    // 2: the Lichess puzzle database is replaced by tactics generated from your own games
    r#"
    DROP TABLE IF EXISTS puzzle_attempts;
    DROP TABLE IF EXISTS puzzle_themes;
    DROP TABLE IF EXISTS puzzles;

    CREATE TABLE tactics(
        id INTEGER PRIMARY KEY,
        game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
        -- index of the opponent's mistake in the game's moves (0 = White's first move)
        ply INTEGER NOT NULL,
        -- position before the mistake, and the mistake followed by the solution (UCI)
        fen TEXT NOT NULL,
        moves TEXT NOT NULL,
        position_key TEXT NOT NULL,
        solver TEXT NOT NULL,
        kind TEXT NOT NULL,
        cp INTEGER NOT NULL,
        themes TEXT NOT NULL,
        -- 'you': the library owner had this chance; 'opponent': the owner made the mistake
        owner TEXT,
        -- what was actually played instead of the solution's first move (NULL if it was found)
        played TEXT,
        version INTEGER NOT NULL,
        created_at INTEGER NOT NULL,
        attempts INTEGER NOT NULL DEFAULT 0,
        solved INTEGER NOT NULL DEFAULT 0,
        streak INTEGER NOT NULL DEFAULT 0,
        due_at INTEGER NOT NULL DEFAULT 0,
        last_at INTEGER,
        UNIQUE(game_id, ply)
    );
    CREATE INDEX tactics_due ON tactics(due_at);
    CREATE INDEX tactics_key ON tactics(position_key);

    CREATE TABLE tactic_attempts(
        id INTEGER PRIMARY KEY,
        tactic_id INTEGER NOT NULL REFERENCES tactics(id) ON DELETE CASCADE,
        solved INTEGER NOT NULL,
        mistakes INTEGER NOT NULL,
        hints INTEGER NOT NULL,
        duration_ms INTEGER NOT NULL,
        attempted_at INTEGER NOT NULL
    );
    CREATE INDEX tactic_attempts_tactic ON tactic_attempts(tactic_id);

    CREATE TABLE tactic_scans(
        game_id INTEGER PRIMARY KEY REFERENCES games(id) ON DELETE CASCADE,
        version INTEGER NOT NULL,
        found INTEGER NOT NULL,
        scanned_at INTEGER NOT NULL
    );
    "#,
];

pub struct Db {
    path: PathBuf,
    conn: Mutex<Connection>,
}

pub fn open_connection(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    Ok(conn)
}

pub fn migrate(conn: &mut Connection) -> Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql).with_context(|| format!("applying migration {}", index + 1))?;
        tx.pragma_update(None, "user_version", (index + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let mut conn = open_connection(path)?;
        migrate(&mut conn)?;
        Ok(Db { path: path.to_path_buf(), conn: Mutex::new(conn) })
    }

    pub fn in_memory() -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate(&mut conn)?;
        Ok(Db { path: PathBuf::from(":memory:"), conn: Mutex::new(conn) })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn conn(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves SQLite itself consistent (every write is
        // transactional), so recovering the guard is safe.
        self.conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn setting(&self, key: &str) -> Option<String> {
        self.conn()
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    pub fn setting_json<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.setting(key).and_then(|v| serde_json::from_str(&v).ok())
    }

    pub fn set_setting_json<T: serde::Serialize>(&self, key: &str, value: &T) -> Result<()> {
        self.set_setting(key, &serde_json::to_string(value)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_once_and_are_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        {
            let db = Db::open(&path).unwrap();
            db.set_setting("a", "1").unwrap();
        }
        let db = Db::open(&path).unwrap();
        assert_eq!(db.setting("a").as_deref(), Some("1"));
        let version: i64 = db.conn().pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(version as usize, MIGRATIONS.len());
    }

    #[test]
    fn settings_round_trip_json() {
        let db = Db::in_memory().unwrap();
        db.set_setting_json("x", &vec![1, 2, 3]).unwrap();
        assert_eq!(db.setting_json::<Vec<i32>>("x"), Some(vec![1, 2, 3]));
        db.set_setting_json("x", &vec![4]).unwrap();
        assert_eq!(db.setting_json::<Vec<i32>>("x"), Some(vec![4]));
    }
}
