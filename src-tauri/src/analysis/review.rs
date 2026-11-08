//! Whole-game review: evaluate every position of the mainline (reusing cached
//! evaluations), then classify every move and summarise each side.

use super::classify::{self, Classification, ClassifyConfig, MoveFacts, Verdict};
use crate::chess;
use crate::db::Db;
use crate::engine::actor::{EngineHandle, PvLine, SearchLimit, SearchRequest, Snapshot};
use crate::engine::uci::Score;
use crate::paths::now;
use anyhow::{bail, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use shakmaty::{Chess, Color, Position};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReviewSettings {
    pub depth: Option<u32>,
    pub movetime_ms: Option<u64>,
    pub multipv: u32,
    pub classify: ClassifyConfig,
}

impl Default for ReviewSettings {
    fn default() -> Self {
        ReviewSettings { depth: Some(16), movetime_ms: None, multipv: 2, classify: ClassifyConfig::default() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionEval {
    pub fen: String,
    pub depth: u32,
    pub lines: Vec<PvLine>,
    pub terminal: Option<String>,
    pub cached: bool,
}

impl PositionEval {
    pub fn score(&self) -> Score {
        self.lines.first().map(|l| l.score).unwrap_or(Score::Cp(0))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveReview {
    pub ply: usize,
    pub color: String,
    pub san: String,
    pub uci: String,
    pub best_san: Option<String>,
    pub best_uci: Option<String>,
    pub best_line: Vec<String>,
    /// White-POV engine evaluation of the position before and after the move.
    pub eval_before: Score,
    pub eval_after: Score,
    #[serde(flatten)]
    pub verdict: Verdict,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SideSummary {
    pub acpl: f64,
    pub accuracy: f64,
    pub moves: usize,
    pub counts: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurningPoint {
    pub ply: usize,
    /// Change in White's winning chances caused by this move (percentage points).
    pub swing: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameReport {
    pub engine: String,
    pub settings: ReviewSettings,
    pub start_fen: String,
    pub positions: Vec<PositionEval>,
    pub moves: Vec<MoveReview>,
    pub white: SideSummary,
    pub black: SideSummary,
    pub turning_points: Vec<TurningPoint>,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ReviewEvent {
    #[serde(rename_all = "camelCase")]
    Progress { done: usize, total: usize, cached: usize, ply: usize, score: Score },
    #[serde(rename_all = "camelCase")]
    Done { report: Box<GameReport> },
    #[serde(rename_all = "camelCase")]
    Error { message: String },
    Cancelled,
}

fn cached_eval(db: &Db, key: &str, engine: &str, depth: u32, multipv: u32) -> Option<(u32, Vec<PvLine>)> {
    let row: Option<(u32, u32, String)> = db
        .conn()
        .query_row("SELECT depth, multipv, lines FROM evals WHERE position=?1 AND engine=?2", params![key, engine], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()
        .ok()
        .flatten();
    let (cached_depth, cached_multipv, lines) = row?;
    if cached_depth < depth || cached_multipv < multipv {
        return None;
    }
    serde_json::from_str(&lines).ok().map(|l| (cached_depth, l))
}

pub fn store_eval(db: &Db, key: &str, engine: &str, depth: u32, multipv: u32, lines: &[PvLine]) -> Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    db.conn().execute(
        "INSERT INTO evals(position,engine,depth,multipv,lines,updated_at) VALUES(?1,?2,?3,?4,?5,?6)
         ON CONFLICT(position,engine) DO UPDATE SET depth=excluded.depth, multipv=excluded.multipv, lines=excluded.lines, updated_at=excluded.updated_at
         WHERE excluded.depth > evals.depth OR (excluded.depth = evals.depth AND excluded.multipv >= evals.multipv)",
        params![key, engine, depth, multipv, serde_json::to_string(lines)?, now()],
    )?;
    Ok(())
}

pub fn cached_lines(db: &Db, key: &str, engine: &str) -> Option<(u32, Vec<PvLine>)> {
    cached_eval(db, key, engine, 0, 1)
}

/// Evaluates every position, then builds the report.
pub async fn run(
    db: &Db,
    engine: &EngineHandle,
    start_fen: Option<&str>,
    moves: &[String],
    settings: &ReviewSettings,
    cancel: &AtomicBool,
    mut emit: impl FnMut(ReviewEvent),
) -> Result<GameReport> {
    let positions = chess::replay(start_fen, moves)?;
    if moves.is_empty() {
        bail!("This game has no moves to review.");
    }
    let engine_key = engine.identity.name.clone();
    let multipv = settings.multipv.clamp(1, 5);
    let min_depth = settings.depth.unwrap_or(1);
    let limit = SearchLimit { depth: settings.depth, movetime_ms: settings.movetime_ms, nodes: None };
    let limit = if limit.is_infinite() { SearchLimit { depth: Some(16), ..Default::default() } } else { limit };
    let total = positions.len();
    let mut evals: Vec<PositionEval> = Vec::with_capacity(total);
    let mut cached_count = 0;
    for (index, pos) in positions.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            bail!("cancelled");
        }
        let key = chess::position_key(pos);
        let fen = chess::to_fen(pos);
        let status = chess::status(pos);
        let eval = if status.over {
            let request = SearchRequest { search_id: index as u64, fen: Some(fen.clone()), moves: vec![], multipv, limit: limit.clone() };
            let snap = engine.search_once(request).await?;
            PositionEval { fen, depth: 0, lines: snap.lines, terminal: snap.terminal, cached: false }
        } else if let Some((depth, lines)) = cached_eval(db, &key, &engine_key, min_depth, multipv.min(pos.legal_moves().len() as u32)) {
            cached_count += 1;
            PositionEval { fen, depth, lines, terminal: None, cached: true }
        } else {
            let request = SearchRequest { search_id: index as u64, fen: Some(fen.clone()), moves: vec![], multipv, limit: limit.clone() };
            let snap: Snapshot = engine.search_once(request).await?;
            if snap.lines.is_empty() {
                bail!("{} returned no evaluation for position {}", engine_key, index + 1);
            }
            store_eval(db, &key, &engine_key, snap.depth, snap.lines.len() as u32, &snap.lines)?;
            PositionEval { fen, depth: snap.depth, lines: snap.lines, terminal: None, cached: false }
        };
        emit(ReviewEvent::Progress { done: index + 1, total, cached: cached_count, ply: index, score: eval.score() });
        evals.push(eval);
    }
    Ok(build_report(&engine_key, settings, &positions, moves, evals))
}

pub fn build_report(engine: &str, settings: &ReviewSettings, positions: &[Chess], moves: &[String], evals: Vec<PositionEval>) -> GameReport {
    let mut reviews = Vec::with_capacity(moves.len());
    for (i, uci) in moves.iter().enumerate() {
        let before = &positions[i];
        let Ok(played) = chess::parse_uci(before, uci) else { continue };
        let facts = MoveFacts { before, played, lines: &evals[i].lines, after: evals[i + 1].score() };
        let Some(verdict) = classify::classify(&facts, &settings.classify) else { continue };
        let best = evals[i].lines.first();
        reviews.push(MoveReview {
            ply: i + 1,
            color: chess::color_name(before.turn()).into(),
            san: chess::san_string(before, played),
            uci: chess::uci_string(played),
            best_san: best.and_then(|l| l.san.first().cloned()),
            best_uci: best.and_then(|l| l.uci.first().cloned()),
            best_line: best.map(|l| l.san.iter().take(10).cloned().collect()).unwrap_or_default(),
            eval_before: evals[i].score(),
            eval_after: evals[i + 1].score(),
            verdict,
        });
    }
    let summarize = |color: &str| {
        let mine: Vec<&MoveReview> = reviews.iter().filter(|r| r.color == color).collect();
        let mut summary = SideSummary { moves: mine.len(), ..Default::default() };
        if !mine.is_empty() {
            summary.acpl = mine.iter().map(|r| r.verdict.cp_loss as f64).sum::<f64>() / mine.len() as f64;
            summary.accuracy = mine.iter().map(|r| r.verdict.accuracy).sum::<f64>() / mine.len() as f64;
        }
        for r in &mine {
            let key = serde_json::to_value(r.verdict.classification).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            *summary.counts.entry(key).or_default() += 1;
        }
        summary
    };
    let white = summarize("white");
    let black = summarize("black");
    let mut turning: Vec<TurningPoint> = reviews
        .iter()
        .filter(|r| r.verdict.classification >= Classification::Mistake)
        .map(|r| TurningPoint {
            ply: r.ply,
            swing: classify::win_percent(r.eval_after) - classify::win_percent(r.eval_before),
        })
        .collect();
    turning.sort_by(|a, b| b.swing.abs().total_cmp(&a.swing.abs()));
    turning.truncate(5);
    turning.sort_by_key(|t| t.ply);
    GameReport {
        engine: engine.to_string(),
        settings: settings.clone(),
        start_fen: chess::to_fen(&positions[0]),
        positions: evals,
        moves: reviews,
        white,
        black,
        turning_points: turning,
        created_at: now(),
    }
}

pub fn save(db: &Db, game_id: i64, report: &GameReport) -> Result<()> {
    db.conn().execute(
        "INSERT INTO analyses(game_id,engine,settings,report,created_at) VALUES(?1,?2,?3,?4,?5)
         ON CONFLICT(game_id) DO UPDATE SET engine=excluded.engine, settings=excluded.settings, report=excluded.report, created_at=excluded.created_at",
        params![game_id, report.engine, serde_json::to_string(&report.settings)?, serde_json::to_string(report)?, report.created_at],
    )?;
    Ok(())
}

pub fn load(db: &Db, game_id: i64) -> Result<Option<GameReport>> {
    let text: Option<String> =
        db.conn().query_row("SELECT report FROM analyses WHERE game_id=?1", [game_id], |r| r.get(0)).optional()?;
    Ok(text.and_then(|t| serde_json::from_str(&t).ok()))
}

pub fn mover(pos: &Chess) -> Color {
    pos.turn()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(score: Score, best: &str, san: &str) -> PositionEval {
        PositionEval {
            fen: String::new(),
            depth: 20,
            lines: vec![PvLine { multipv: 1, depth: 20, score, raw_score: score, wdl: None, san: vec![san.into()], uci: vec![best.into()] }],
            terminal: None,
            cached: false,
        }
    }

    #[test]
    fn report_summarises_both_sides() {
        let moves: Vec<String> = ["e2e4", "e7e5", "d1h5", "g8f6"].map(String::from).to_vec();
        let positions = chess::replay(None, &moves).unwrap();
        let evals = vec![
            eval(Score::Cp(30), "e2e4", "e4"),
            eval(Score::Cp(30), "e7e5", "e5"),
            eval(Score::Cp(30), "g1f3", "Nf3"),
            eval(Score::Cp(-10), "b8c6", "Nc6"),
            eval(Score::Mate(1), "h5f7", "Qxf7#"),
        ];
        let report = build_report("Test", &ReviewSettings::default(), &positions, &moves, evals);
        assert_eq!(report.moves.len(), 4);
        assert_eq!(report.moves[0].verdict.classification, Classification::Best);
        assert_eq!(report.moves[3].verdict.classification, Classification::Blunder);
        assert!(report.moves[3].verdict.reasons.iter().any(|r| r.contains("Allows a forced mate")));
        assert_eq!(report.black.counts.get("blunder"), Some(&1));
        assert_eq!(report.white.moves, 2);
        assert!(report.black.acpl > report.white.acpl);
        assert_eq!(report.turning_points.len(), 1);
        assert_eq!(report.turning_points[0].ply, 4);
    }

    #[test]
    fn eval_cache_keeps_deepest() {
        let db = Db::in_memory().unwrap();
        let line = |cp| vec![PvLine { multipv: 1, depth: 0, score: Score::Cp(cp), raw_score: Score::Cp(cp), wdl: None, san: vec![], uci: vec!["e2e4".into()] }];
        store_eval(&db, "k", "SF", 20, 2, &line(10)).unwrap();
        store_eval(&db, "k", "SF", 12, 2, &line(99)).unwrap();
        let (depth, lines) = cached_lines(&db, "k", "SF").unwrap();
        assert_eq!(depth, 20);
        assert_eq!(lines[0].score, Score::Cp(10));
        assert!(cached_eval(&db, "k", "SF", 22, 1).is_none());
        assert!(cached_eval(&db, "k", "SF", 18, 3).is_none());
        assert!(cached_eval(&db, "k", "Other", 1, 1).is_none());
    }
}
