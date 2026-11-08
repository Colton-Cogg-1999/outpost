// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Tactic generator. A port of lichess-puzzler's generator/generator.py
// (https://github.com/ornicar/lichess-puzzler, AGPL-3.0, by Thibault Duplessis and
// contributors), the program that built the Lichess puzzle database, to Rust.
//
// Differences from upstream, all deliberate for a single user's own games:
// * every candidate in a game is kept, not just the first one;
// * game-quality tiers are not used (your games are the point), so mate-in-one
//   and two-move puzzles are allowed;
// * no tablebase lookups (OUTPOST works offline);
// * search limits are smaller, for a desktop machine.

use super::util::{count_mates, is_up_in_material, material_diff, maximum_castling_rights};
use crate::chess;
use crate::engine::actor::{EngineHandle, SearchLimit, SearchRequest};
use crate::engine::uci::Score;
use anyhow::Result;
use shakmaty::{Chess, Color, Move, Position};
use std::cmp::Ordering;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

/// Bump when the algorithm changes, so games are rescanned.
pub const VERSION: i64 = 1;

/// "Mate soon": anything quicker than mate in 15.
const MATE_SOON: Score = Score::Mate(15);

/// python-chess score ordering, from the point of view the score is given in:
/// mates for us (sooner is better) > any centipawns > mates against us (later is better).
pub fn cmp_score(a: Score, b: Score) -> Ordering {
    fn key(s: Score) -> (i8, i64) {
        match s {
            Score::Mate(n) if n > 0 => (2, -(n as i64)),
            Score::Cp(cp) => (1, cp as i64),
            // Being mated: later is better, and mated-now (0) is worst.
            Score::Mate(n) => (0, (n as i64).abs()),
        }
    }
    key(a).cmp(&key(b))
}

fn gt(a: Score, b: Score) -> bool {
    cmp_score(a, b) == Ordering::Greater
}
fn lt(a: Score, b: Score) -> bool {
    cmp_score(a, b) == Ordering::Less
}
fn ge(a: Score, b: Score) -> bool {
    !lt(a, b)
}

/// Winning chances from -1 to 1 (lichess-org/lila#11148).
pub fn win_chances(s: Score) -> f64 {
    match s {
        Score::Mate(n) => {
            if n > 0 {
                1.0
            } else {
                -1.0
            }
        }
        Score::Cp(cp) => 2.0 / (1.0 + (-0.00368208 * cp as f64).exp()) - 1.0,
    }
}

#[derive(Clone, Debug)]
pub struct EngineMove {
    pub mv: Move,
    /// From the point of view of the puzzle's winner.
    pub score: Score,
}

#[derive(Clone, Debug)]
pub struct Pair {
    pub best: EngineMove,
    pub second: Option<EngineMove>,
}

/// How strict the generator is.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Strictness {
    /// Upstream's rules, as used for the Lichess puzzle database.
    Lichess,
    /// Looser rules for one person's games: more tactics, still only-move ones.
    Personal,
}

#[derive(Clone, Copy, Debug)]
pub struct Rules {
    /// How much better (in winning chances, -1..1) the best move must be than the second.
    pub attack_margin: f64,
    /// How much the mistake must swing the winning chances.
    pub jump: f64,
    /// Accept a single winning move (e.g. taking a piece that is the only good move).
    pub one_move: bool,
    /// Keep mates even when the solver is already ahead in material.
    pub mates_when_ahead: bool,
}

impl Strictness {
    pub fn rules(self) -> Rules {
        match self {
            Strictness::Lichess => Rules { attack_margin: 0.7, jump: 0.6, one_move: false, mates_when_ahead: false },
            Strictness::Personal => Rules { attack_margin: 0.45, jump: 0.5, one_move: true, mates_when_ahead: true },
        }
    }
}

#[derive(Clone, Debug)]
pub struct Limits {
    pub pair: SearchLimit,
    pub mate_defense: SearchLimit,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            pair: SearchLimit { depth: Some(50), movetime_ms: Some(3000), nodes: Some(3_000_000) },
            mate_defense: SearchLimit { depth: Some(15), movetime_ms: Some(1500), nodes: Some(1_000_000) },
        }
    }
}

/// What the generator needs from an engine: the best `multipv` moves and their
/// scores from the side to move's point of view.
#[allow(async_fn_in_trait)]
pub trait Analyst {
    async fn analyse(&mut self, pos: &Chess, multipv: u32, limit: &SearchLimit) -> Result<Vec<(Move, Score)>>;
}

impl Analyst for &EngineHandle {
    async fn analyse(&mut self, pos: &Chess, multipv: u32, limit: &SearchLimit) -> Result<Vec<(Move, Score)>> {
        let request = SearchRequest { search_id: 0, fen: Some(chess::to_fen(pos)), moves: vec![], multipv, limit: limit.clone() };
        let snapshot = self.search_once(request).await?;
        let mut out = Vec::new();
        for line in snapshot.lines {
            if let Some(first) = line.uci.first() {
                if let Ok(m) = chess::parse_uci(pos, first) {
                    out.push((m, line.raw_score));
                }
            }
        }
        Ok(out)
    }
}

pub struct Found {
    /// Index of the opponent's mistake in the game's move list.
    pub ply: usize,
    /// The mistake followed by the solution (UCI).
    pub moves: Vec<Move>,
    pub kind: &'static str,
    /// Upstream's `cp`: the final evaluation for the solver, 999999999 for mates.
    pub cp: i64,
}

pub struct Generator<'a, A: Analyst> {
    pub engine: A,
    pub limits: Limits,
    pub rules: Rules,
    pub cancel: &'a AtomicBool,
}

fn pov(score: Score, side_to_move: Color, winner: Color) -> Score {
    if side_to_move == winner { score } else { score.negate() }
}

impl<A: Analyst> Generator<'_, A> {
    fn cancelled(&self) -> Result<()> {
        if self.cancel.load(AtomicOrdering::Relaxed) {
            anyhow::bail!("cancelled");
        }
        Ok(())
    }

    async fn next_move_pair(&mut self, pos: &Chess, winner: Color) -> Result<Option<Pair>> {
        self.cancelled()?;
        let lines = self.engine.analyse(pos, 2, &self.limits.pair).await?;
        let mut it = lines.into_iter().map(|(mv, s)| EngineMove { mv, score: pov(s, pos.turn(), winner) });
        let Some(best) = it.next() else { return Ok(None) };
        Ok(Some(Pair { best, second: it.next() }))
    }

    async fn is_valid_mate_in_one(&mut self, pos: &Chess, pair: &Pair, winner: Color) -> Result<bool> {
        if pair.best.score != Score::Mate(1) {
            return Ok(false);
        }
        const NON_MATE_WIN_THRESHOLD: f64 = 0.6;
        let Some(second) = &pair.second else { return Ok(true) };
        if win_chances(second.score) <= NON_MATE_WIN_THRESHOLD {
            return Ok(true);
        }
        if second.score == Score::Mate(1) {
            // More than one mate in one: is the best non-mating move bad enough?
            let mates = count_mates(pos) as u32;
            let lines = self.engine.analyse(pos, mates + 1, &self.limits.pair).await?;
            if let Some((_, last)) = lines.last() {
                let last = pov(*last, pos.turn(), winner);
                if lt(last, Score::Mate(1)) && win_chances(last) > NON_MATE_WIN_THRESHOLD {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        Ok(false)
    }

    /// Is `pair.best` the only continuation?
    async fn is_valid_attack(&mut self, pos: &Chess, pair: &Pair, winner: Color) -> Result<bool> {
        let Some(second) = &pair.second else { return Ok(true) };
        Ok(self.is_valid_mate_in_one(pos, pair, winner).await? || win_chances(pair.best.score) > win_chances(second.score) + self.rules.attack_margin)
    }

    async fn next_pair(&mut self, pos: &Chess, winner: Color) -> Result<Option<Pair>> {
        let Some(pair) = self.next_move_pair(pos, winner).await? else { return Ok(None) };
        if pos.turn() == winner && !self.is_valid_attack(pos, &pair, winner).await? {
            tracing::debug!("not the only good move: best {:?}, second {:?}", pair.best.score, pair.second.as_ref().map(|s| s.score));
            return Ok(None);
        }
        Ok(Some(pair))
    }

    async fn cook_mate(&mut self, start: &Chess, winner: Color) -> Result<Option<Vec<Move>>> {
        let mut pos = start.clone();
        let mut moves = Vec::new();
        loop {
            if pos.is_game_over() {
                return Ok(pos.is_checkmate().then_some(moves));
            }
            if moves.len() > 40 {
                return Ok(None);
            }
            let mv = if pos.turn() == winner {
                let Some(pair) = self.next_pair(&pos, winner).await? else { return Ok(None) };
                if lt(pair.best.score, MATE_SOON) {
                    // Best move is not a mate; probably not searching deep enough.
                    return Ok(None);
                }
                pair.best.mv
            } else {
                self.cancelled()?;
                let lines = self.engine.analyse(&pos, 1, &self.limits.mate_defense).await?;
                let Some((mv, _)) = lines.into_iter().next() else { return Ok(None) };
                mv
            };
            moves.push(mv);
            pos.play_unchecked(mv);
        }
    }

    async fn cook_advantage(&mut self, start: &Chess, history: &[String], winner: Color) -> Result<Option<Vec<Pair>>> {
        let mut pos = start.clone();
        // `history` ends with `start`; python-chess `is_repetition(2)` counts the current position too.
        let mut seen: Vec<String> = history.to_vec();
        let mut solution: Vec<Pair> = Vec::new();
        loop {
            let key = chess::position_key(&pos);
            if seen.iter().filter(|k| **k == key).count() >= 2 {
                // Repetition: cancel.
                return Ok(None);
            }
            if pos.is_game_over() || solution.len() > 40 {
                return Ok(Some(solution));
            }
            let Some(pair) = self.next_pair(&pos, winner).await? else { return Ok(Some(solution)) };
            if lt(pair.best.score, Score::Cp(200)) {
                // Not winning enough.
                return Ok(None);
            }
            pos.play_unchecked(pair.best.mv);
            seen.push(chess::position_key(&pos));
            solution.push(pair);
        }
    }

    /// Port of `analyze_position`: Ok(Some) is a puzzle, Ok(None) means keep going,
    /// and the returned score is this position's evaluation for the side to move.
    async fn analyze_position(&mut self, pos: &Chess, history: &[String], prev_score: Score, score: Score) -> Result<Option<(Vec<Move>, &'static str, i64)>> {
        let winner = pos.turn();
        if pos.legal_moves().len() < 2 {
            return Ok(None);
        }
        if gt(prev_score, Score::Cp(300)) && lt(score, MATE_SOON) {
            // Too much of a winning position to start with.
            return Ok(None);
        }
        if is_up_in_material(pos.board(), winner) && !(self.rules.mates_when_ahead && gt(score, MATE_SOON)) {
            tracing::debug!("already up in material: {score:?}");
            return Ok(None);
        }
        if gt(score, MATE_SOON) {
            let Some(solution) = self.cook_mate(pos, winner).await? else {
                tracing::debug!("mate {score:?}: no unique mating line");
                return Ok(None);
            };
            if solution.is_empty() {
                return Ok(None);
            }
            return Ok(Some((solution, "mate", 999_999_999)));
        }
        if ge(score, Score::Cp(200)) && win_chances(score) > win_chances(prev_score) + self.rules.jump {
            if lt(score, Score::Cp(400)) && material_diff(pos.board(), winner) > -1 {
                tracing::debug!("advantage {score:?}: not clearly winning");
                return Ok(None);
            }
            let Some(mut solution) = self.cook_advantage(pos, history, winner).await? else {
                tracing::debug!("advantage {score:?}: line not winning enough, or repeats");
                return Ok(None);
            };
            // End on the solver's move, and drop a final move that was the only legal one.
            while !solution.is_empty() && (solution.len() % 2 == 0 || solution.last().is_some_and(|p| p.second.is_none())) {
                solution.pop();
            }
            // A single winning move counts (Personal rules) when it wins at least ~3 pawns.
            let one_move_ok = self.rules.one_move && solution.len() == 1 && ge(solution[0].best.score, Score::Cp(300));
            if solution.is_empty() || (solution.len() == 1 && !one_move_ok) {
                tracing::debug!("advantage {score:?}: one-mover");
                return Ok(None);
            }
            let cp = match solution.last().map(|p| p.best.score) {
                Some(Score::Cp(cp)) => cp as i64,
                _ => 999_999_998,
            };
            return Ok(Some((solution.iter().map(|p| p.best.mv).collect(), "advantage", cp)));
        }
        Ok(None)
    }

    /// Port of `analyze_game`. `evals[i]` is the White-POV evaluation of `positions[i]`.
    pub async fn analyze_game(&mut self, positions: &[Chess], moves: &[Move], evals: &[Score], mut on_candidate: impl FnMut(usize)) -> Result<Vec<Found>> {
        let mut found = Vec::new();
        let mut prev_score = Score::Cp(20);
        let mut seen: HashSet<String> = HashSet::new();
        let mut history: Vec<String> = vec![chess::position_key(&positions[0])];
        let mut skip_until_irreversible = false;
        // Positions on an already-found solution: the game following it is not a new tactic.
        let mut solved_line: HashSet<String> = HashSet::new();
        for (i, mv) in moves.iter().enumerate() {
            self.cancelled()?;
            let before = &positions[i];
            let pos = &positions[i + 1];
            let key = chess::position_key(pos);
            history.push(key.clone());
            if skip_until_irreversible {
                if is_irreversible(before, pos, *mv) {
                    skip_until_irreversible = false;
                    seen.clear();
                } else {
                    continue;
                }
            }
            if !seen.insert(key) {
                skip_until_irreversible = true;
                continue;
            }
            if pos.castles().castling_rights() != maximum_castling_rights(pos.board()) {
                continue;
            }
            let score = pov(evals[i + 1], Color::White, pos.turn());
            // A forced move is not a mistake, and neither is following a known solution.
            if before.legal_moves().len() < 2 || solved_line.contains(&chess::position_key(pos)) {
                prev_score = score.negate();
                continue;
            }
            tracing::debug!("ply {} {:?}", i + 1, score);
            match self.analyze_position(pos, &history, prev_score, score).await {
                Ok(Some((solution, kind, cp))) => {
                    on_candidate(i);
                    let mut cursor = pos.clone();
                    for m in &solution {
                        cursor.play_unchecked(*m);
                        solved_line.insert(chess::position_key(&cursor));
                    }
                    let mut line = vec![*mv];
                    line.extend(solution);
                    found.push(Found { ply: i, moves: line, kind, cp });
                }
                Ok(None) => {}
                Err(e) if e.to_string() == "cancelled" => return Err(e),
                Err(e) => tracing::warn!("tactic search failed at ply {}: {e:#}", i + 1),
            }
            prev_score = score.negate();
        }
        Ok(found)
    }
}

/// A capture, a pawn move, or a change of castling rights: no earlier position can recur.
fn is_irreversible(before: &Chess, after: &Chess, mv: Move) -> bool {
    mv.is_capture() || mv.role() == shakmaty::Role::Pawn || before.castles().castling_rights() != after.castles().castling_rights()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_order_matches_python_chess() {
        assert!(gt(Score::Mate(1), Score::Mate(2)));
        assert!(gt(Score::Mate(15), Score::Cp(5000)));
        assert!(gt(Score::Cp(-5000), Score::Mate(-30)));
        assert!(gt(Score::Mate(-30), Score::Mate(-1)));
        assert!(gt(Score::Mate(-1), Score::Mate(0)));
        assert!(lt(Score::Cp(300), MATE_SOON));
        assert!(gt(Score::Mate(3), MATE_SOON));
        assert!(!gt(MATE_SOON, MATE_SOON));
        assert!(ge(Score::Mate(1), Score::Mate(1)));
    }

    #[test]
    fn win_chances_curve() {
        assert!((win_chances(Score::Cp(0))).abs() < 1e-9);
        assert!(win_chances(Score::Cp(200)) > 0.3 && win_chances(Score::Cp(200)) < 0.4);
        assert_eq!(win_chances(Score::Mate(-3)), -1.0);
    }

    /// A scripted engine: answers come from a table keyed by position.
    struct Scripted(Vec<(String, Vec<(&'static str, Score)>)>);

    impl Analyst for Scripted {
        async fn analyse(&mut self, pos: &Chess, multipv: u32, _limit: &SearchLimit) -> Result<Vec<(Move, Score)>> {
            let key = chess::position_key(pos);
            let Some((_, lines)) = self.0.iter().find(|(k, _)| *k == key) else {
                // Unknown positions: the engine prefers the first legal move, slightly.
                return Ok(pos.legal_moves().into_iter().take(multipv as usize).map(|m| (m, Score::Cp(0))).collect());
            };
            Ok(lines.iter().take(multipv as usize).map(|(uci, s)| (chess::parse_uci(pos, uci).unwrap(), *s)).collect())
        }
    }

    #[test]
    fn finds_the_mate_after_a_blunder() {
        // 1. e4 e5 2. Bc4 Nc6 3. Qh5 Nf6?? 4. Qxf7#
        let ucis: Vec<String> = ["e2e4", "e7e5", "f1c4", "b8c6", "d1h5", "g8f6"].iter().map(|s| s.to_string()).collect();
        let positions = chess::replay(None, &ucis).unwrap();
        let moves: Vec<Move> = ucis.iter().enumerate().map(|(i, u)| chess::parse_uci(&positions[i], u).unwrap()).collect();
        let mut evals = vec![Score::Cp(20); positions.len()];
        evals[6] = Score::Mate(1); // White to move, mates in one.
        let after = chess::position_key(&positions[6]);
        let engine = Scripted(vec![(after, vec![("h5f7", Score::Mate(1)), ("c4f7", Score::Cp(150))])]);
        let cancel = AtomicBool::new(false);
        let mut generator = Generator { engine, limits: Limits::default(), rules: Strictness::Lichess.rules(), cancel: &cancel };
        let found = futures_util::FutureExt::now_or_never(generator.analyze_game(&positions, &moves, &evals, |_| {})).unwrap().unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].ply, 5);
        assert_eq!(found[0].kind, "mate");
        assert_eq!(found[0].moves.iter().map(|m| chess::uci_string(*m)).collect::<Vec<_>>(), ["g8f6", "h5f7"]);
    }

    #[test]
    fn ignores_quiet_games() {
        let ucis: Vec<String> = ["e2e4", "e7e5", "g1f3", "b8c6"].iter().map(|s| s.to_string()).collect();
        let positions = chess::replay(None, &ucis).unwrap();
        let moves: Vec<Move> = ucis.iter().enumerate().map(|(i, u)| chess::parse_uci(&positions[i], u).unwrap()).collect();
        let evals = vec![Score::Cp(25); positions.len()];
        let cancel = AtomicBool::new(false);
        let mut generator = Generator { engine: Scripted(vec![]), limits: Limits::default(), rules: Strictness::Personal.rules(), cancel: &cancel };
        let found = futures_util::FutureExt::now_or_never(generator.analyze_game(&positions, &moves, &evals, |_| {})).unwrap().unwrap();
        assert!(found.is_empty());
    }
}
