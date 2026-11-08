//! Move classification. Everything here is a *heuristic* layered on top of measured
//! engine output; the UI labels it as such. The measured facts (engine scores, best
//! move, principal variations) are stored alongside so users can always see why.
//!
//! The core quantity is **expected-score loss**: the drop, in percentage points, of
//! the mover's winning chances between the engine's best move and the move played.
//! Centipawns are converted with the logistic curve Lichess publishes
//! (`50 + 50 * (2 / (1 + exp(-0.00368208 * cp)) - 1)`), so losing 200cp when +900 costs
//! almost nothing while losing 200cp from equality is serious. Mate scores map to 0/100.
//! See docs/classification.md for the full specification.

use crate::chess::{self, piece_value, see};
use crate::engine::actor::PvLine;
use crate::engine::uci::Score;
use serde::{Deserialize, Serialize};
use shakmaty::{Chess, Color, Move, Position, Role};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Classification {
    Brilliant,
    Great,
    Best,
    Good,
    Inaccuracy,
    Mistake,
    Blunder,
    Forced,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ClassifyConfig {
    /// Expected-score loss (percentage points) at or below which a move counts as best.
    pub best_tolerance: f64,
    pub inaccuracy: f64,
    pub mistake: f64,
    pub blunder: f64,
    /// "Great": the best move must beat the engine's second choice by at least this much.
    pub great_gap: f64,
    /// "Great"/"Brilliant" are not awarded when the mover was already this likely to win.
    pub decided_above: f64,
    /// "Brilliant": the sacrifice must give up at least this much material (pawns), net.
    pub sacrifice_min: i32,
    /// "Brilliant": the sacrifice must lose no more than this much expected score.
    pub brilliant_max_loss: f64,
    /// "Brilliant": after the sacrifice the mover's winning chances must be at least this.
    pub brilliant_min_after: f64,
}

impl Default for ClassifyConfig {
    fn default() -> Self {
        ClassifyConfig {
            best_tolerance: 1.0,
            inaccuracy: 5.0,
            mistake: 10.0,
            blunder: 15.0,
            great_gap: 10.0,
            decided_above: 95.0,
            sacrifice_min: 2,
            brilliant_max_loss: 2.0,
            brilliant_min_after: 45.0,
        }
    }
}

/// White's winning chances in percent for a White-POV score.
pub fn win_percent(score: Score) -> f64 {
    match score {
        Score::Mate(n) if n > 0 => 100.0,
        Score::Mate(n) if n < 0 => 0.0,
        Score::Mate(_) => 50.0,
        Score::Cp(cp) => {
            let cp = cp.clamp(-1000, 1000) as f64;
            50.0 + 50.0 * (2.0 / (1.0 + (-0.00368208 * cp).exp()) - 1.0)
        }
    }
}

/// Clamped centipawns for average-centipawn-loss statistics (mate = ±1000).
pub fn clamp_cp(score: Score) -> i32 {
    match score {
        Score::Cp(cp) => cp.clamp(-1000, 1000),
        Score::Mate(n) if n >= 0 => 1000,
        Score::Mate(_) => -1000,
    }
}

/// Lichess's per-move accuracy curve from a win-percentage drop.
pub fn move_accuracy(win_loss: f64) -> f64 {
    (103.1668 * (-0.04354 * win_loss.max(0.0)).exp() - 3.1669).clamp(0.0, 100.0)
}

fn for_mover(white_pov: f64, mover: Color) -> f64 {
    if mover == Color::White { white_pov } else { 100.0 - white_pov }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Verdict {
    pub classification: Classification,
    /// Mover's expected score before (best play) and after the move played, in percent.
    pub win_before: f64,
    pub win_after: f64,
    pub win_loss: f64,
    pub cp_loss: i32,
    pub accuracy: f64,
    /// Human-readable justification of the label, derived from the measurements.
    pub reasons: Vec<String>,
    pub sacrifice: Option<i32>,
}

/// Net material (pawns) the mover offers with `m`: the most the opponent can win by
/// exchanges after the move, minus what the move itself captured. Only counts a piece
/// that the opponent can legally capture right away and that was not already loose.
pub fn sacrifice_value(before: &Chess, m: Move) -> i32 {
    let mover = before.turn();
    let captured = match m {
        Move::Normal { capture: Some(role), .. } => piece_value(role),
        Move::EnPassant { .. } => 1,
        _ => 0,
    };
    let promotion_gain = match m {
        Move::Normal { promotion: Some(role), .. } => piece_value(role) - 1,
        _ => 0,
    };
    let Ok(after) = before.clone().play(m) else { return 0 };
    let legal_capture_targets: Vec<_> = after.capture_moves().iter().map(|c| c.to()).collect();
    let mut worst = 0;
    for sq in after.board().by_color(mover) {
        let Some(role) = after.board().role_at(sq) else { continue };
        if role == Role::King || !legal_capture_targets.contains(&sq) {
            continue;
        }
        let at_risk = see(&after, sq, !mover);
        if at_risk == 0 {
            continue;
        }
        // A piece that was already hanging before the move is not being sacrificed now
        // (unless it is the piece that just moved).
        let moved_here = sq == m.to();
        let already = if moved_here { 0 } else { see(before, sq, !mover) };
        let offered = at_risk - already;
        worst = worst.max(offered);
    }
    (worst - captured - promotion_gain).max(0)
}

pub struct MoveFacts<'a> {
    pub before: &'a Chess,
    pub played: Move,
    /// Engine lines at the position before the move (White POV scores), best first.
    pub lines: &'a [PvLine],
    /// Engine evaluation (White POV) of the position after the move.
    pub after: Score,
}

pub fn classify(facts: &MoveFacts, config: &ClassifyConfig) -> Option<Verdict> {
    let mover = facts.before.turn();
    let best = facts.lines.first()?;
    let played_uci = chess::uci_string(facts.played);
    let is_top = best.uci.first() == Some(&played_uci);
    // When the move played is one of the engine's own candidates, use that line's score:
    // it comes from the same search, so it cannot disagree with the best line by noise.
    let after_score = if is_top {
        best.score
    } else {
        facts
            .lines
            .iter()
            .find(|l| l.uci.first() == Some(&played_uci))
            .map(|l| l.score)
            .unwrap_or(facts.after)
    };
    let win_before = for_mover(win_percent(best.score), mover);
    let win_after = for_mover(win_percent(after_score), mover);
    let win_loss = if is_top { 0.0 } else { (win_before - win_after).max(0.0) };
    let sign = if mover == Color::White { 1 } else { -1 };
    let cp_loss = if is_top { 0 } else { ((clamp_cp(best.score) - clamp_cp(after_score)) * sign).max(0) };
    let accuracy = move_accuracy(win_loss);
    let mut reasons = Vec::new();

    let legal = facts.before.legal_moves().len();
    if legal == 1 {
        reasons.push("Only legal move.".into());
        return Some(Verdict { classification: Classification::Forced, win_before, win_after, win_loss, cp_loss, accuracy, reasons, sacrifice: None });
    }

    let mut classification = if win_loss <= config.best_tolerance {
        if is_top {
            reasons.push("Engine's first choice.".into());
        } else {
            reasons.push(format!("Within {:.1}% of the engine's best move.", config.best_tolerance));
        }
        Classification::Best
    } else if win_loss < config.inaccuracy {
        reasons.push(format!("Gives up {win_loss:.1}% expected score — small enough to be a sound alternative."));
        Classification::Good
    } else if win_loss < config.mistake {
        reasons.push(format!("Gives up {win_loss:.1}% expected score."));
        Classification::Inaccuracy
    } else if win_loss < config.blunder {
        reasons.push(format!("Gives up {win_loss:.1}% expected score."));
        Classification::Mistake
    } else {
        reasons.push(format!("Gives up {win_loss:.1}% expected score."));
        Classification::Blunder
    };

    // Missed mate / allowed mate are worth naming explicitly.
    if let (Score::Mate(n), false) = (best.score, is_top) {
        if (n > 0) == (mover == Color::White) && !matches!(after_score, Score::Mate(m) if (m > 0) == (mover == Color::White)) {
            reasons.push(format!("Missed a forced mate in {}.", n.abs()));
        }
    }
    if let Score::Mate(m) = after_score {
        if (m > 0) != (mover == Color::White) && !matches!(best.score, Score::Mate(n) if (n > 0) != (mover == Color::White)) {
            reasons.push(format!("Allows a forced mate in {}.", m.abs()));
        }
    }

    let undecided = win_before < config.decided_above;
    let mut sacrifice = None;
    if win_loss <= config.brilliant_max_loss && undecided && win_after >= config.brilliant_min_after {
        let offered = sacrifice_value(facts.before, facts.played);
        if offered >= config.sacrifice_min {
            sacrifice = Some(offered);
            classification = Classification::Brilliant;
            reasons.push(format!(
                "Sound sacrifice: offers about {offered} pawn(s) of material by static exchange, yet keeps {win_after:.0}% expected score."
            ));
        }
    }
    if classification == Classification::Best && undecided {
        if let Some(second) = facts.lines.get(1) {
            let second_win = for_mover(win_percent(second.score), mover);
            let gap = win_before - second_win;
            if is_top && gap >= config.great_gap && win_after >= 20.0 {
                classification = Classification::Great;
                reasons.push(format!("Only good move: the engine's next choice ({}) is {gap:.0}% worse.", second.san.first().cloned().unwrap_or_default()));
            }
        }
    }
    Some(Verdict { classification, win_before, win_after, win_loss, cp_loss, accuracy, reasons, sacrifice })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(uci: &str, san: &str, score: Score) -> PvLine {
        PvLine { multipv: 1, depth: 20, score, raw_score: score, wdl: None, san: vec![san.into()], uci: vec![uci.into()] }
    }

    #[test]
    fn win_percent_curve() {
        assert!((win_percent(Score::Cp(0)) - 50.0).abs() < 1e-9);
        assert!(win_percent(Score::Cp(100)) > 58.0 && win_percent(Score::Cp(100)) < 60.0);
        assert!((win_percent(Score::Cp(300)) + win_percent(Score::Cp(-300)) - 100.0).abs() < 1e-9);
        assert_eq!(win_percent(Score::Mate(3)), 100.0);
        assert_eq!(win_percent(Score::Mate(-1)), 0.0);
        assert_eq!(win_percent(Score::Cp(5000)), win_percent(Score::Cp(1000)));
    }

    #[test]
    fn same_cp_loss_means_less_when_already_winning() {
        let pos = Chess::default();
        let m = chess::parse_uci(&pos, "a2a3").unwrap();
        let equal = [line("e2e4", "e4", Score::Cp(20))];
        let v1 = classify(&MoveFacts { before: &pos, played: m, lines: &equal, after: Score::Cp(-230) }, &ClassifyConfig::default()).unwrap();
        let winning = [line("e2e4", "e4", Score::Cp(900))];
        let v2 = classify(&MoveFacts { before: &pos, played: m, lines: &winning, after: Score::Cp(650) }, &ClassifyConfig::default()).unwrap();
        assert_eq!(v1.cp_loss, 250);
        assert_eq!(v2.cp_loss, 250);
        assert!(v1.classification >= Classification::Mistake, "{:?}", v1.classification);
        assert!(v2.classification <= Classification::Inaccuracy, "{:?}", v2.classification);
    }

    #[test]
    fn black_perspective() {
        let pos = chess::replay(None, &["e2e4".to_string()]).unwrap().pop().unwrap();
        let m = chess::parse_uci(&pos, "g7g5").unwrap();
        // Engine says equal; after g5 White is +3: a blunder for Black.
        let lines = [line("e7e5", "e5", Score::Cp(-20))];
        let v = classify(&MoveFacts { before: &pos, played: m, lines: &lines, after: Score::Cp(300) }, &ClassifyConfig::default()).unwrap();
        assert_eq!(v.classification, Classification::Blunder);
        assert!(v.win_before > 50.0, "black is fine before: {}", v.win_before);
    }

    #[test]
    fn top_move_is_best_and_only_move_is_great() {
        let pos = Chess::default();
        let m = chess::parse_uci(&pos, "e2e4").unwrap();
        let lines = [line("e2e4", "e4", Score::Cp(30)), line("d2d4", "d4", Score::Cp(25))];
        let v = classify(&MoveFacts { before: &pos, played: m, lines: &lines, after: Score::Cp(10) }, &ClassifyConfig::default()).unwrap();
        assert_eq!(v.classification, Classification::Best);
        assert_eq!(v.win_loss, 0.0, "noise in the next search is ignored for the top move");

        let lines = [line("e2e4", "e4", Score::Cp(30)), line("d2d4", "d4", Score::Cp(-250))];
        let v = classify(&MoveFacts { before: &pos, played: m, lines: &lines, after: Score::Cp(30) }, &ClassifyConfig::default()).unwrap();
        assert_eq!(v.classification, Classification::Great);
    }

    #[test]
    fn forced_move() {
        // Black king a8, white king b6: Kb8 is the only legal move.
        let pos = chess::parse_fen("k7/8/1K6/8/8/8/8/7R b - - 0 1").unwrap();
        assert_eq!(pos.legal_moves().len(), 1, "fixture must have one legal move");
        let m = pos.legal_moves()[0];
        let lines = [line(&chess::uci_string(m), "Kb8", Score::Mate(-2))];
        let v = classify(&MoveFacts { before: &pos, played: m, lines: &lines, after: Score::Mate(-2) }, &ClassifyConfig::default()).unwrap();
        assert_eq!(v.classification, Classification::Forced);
    }

    #[test]
    fn sacrifice_detection() {
        // Greek gift: Bxh7+ gives up the bishop for a pawn (h7 defended only by the king).
        let pos = chess::parse_fen("r1bq1rk1/pppn1ppp/4p3/3pP3/1b1P4/2NB1N2/PPP2PPP/R2QK2R w KQ - 0 8").unwrap();
        let m = chess::parse_uci(&pos, "d3h7").unwrap();
        assert_eq!(sacrifice_value(&pos, m), 2);
        // A quiet developing move offers nothing.
        let m = chess::parse_uci(&pos, "e1g1").unwrap();
        assert_eq!(sacrifice_value(&pos, m), 0);
        // An even trade is not a sacrifice: Nxd5 exd5.
        let pos = chess::parse_fen("4k3/8/4p3/3n4/8/2N5/8/4K3 w - - 0 1").unwrap();
        let m = chess::parse_uci(&pos, "c3d5").unwrap();
        assert_eq!(sacrifice_value(&pos, m), 0);
    }

    #[test]
    fn brilliant_requires_soundness() {
        let pos = chess::parse_fen("r1bq1rk1/pppn1ppp/4p3/3pP3/1b1P4/2NB1N2/PPP2PPP/R2QK2R w KQ - 0 8").unwrap();
        let m = chess::parse_uci(&pos, "d3h7").unwrap();
        let sound = [line("d3h7", "Bxh7+", Score::Cp(150)), line("e1g1", "O-O", Score::Cp(40))];
        let v = classify(&MoveFacts { before: &pos, played: m, lines: &sound, after: Score::Cp(150) }, &ClassifyConfig::default()).unwrap();
        assert_eq!(v.classification, Classification::Brilliant);
        assert_eq!(v.sacrifice, Some(2));
        // Same sacrifice when the engine thinks it loses: it is just a mistake.
        let unsound = [line("e1g1", "O-O", Score::Cp(40))];
        let v = classify(&MoveFacts { before: &pos, played: m, lines: &unsound, after: Score::Cp(-250) }, &ClassifyConfig::default()).unwrap();
        assert!(v.classification >= Classification::Mistake);
    }

    #[test]
    fn missed_mate_is_reported() {
        let pos = Chess::default();
        let m = chess::parse_uci(&pos, "a2a3").unwrap();
        let lines = [line("e2e4", "e4", Score::Mate(3))];
        let v = classify(&MoveFacts { before: &pos, played: m, lines: &lines, after: Score::Cp(400) }, &ClassifyConfig::default()).unwrap();
        assert!(v.reasons.iter().any(|r| r.contains("Missed a forced mate in 3")), "{:?}", v.reasons);
    }

    #[test]
    fn accuracy_curve() {
        assert!((move_accuracy(0.0) - 100.0).abs() < 0.01);
        assert!(move_accuracy(10.0) < 70.0);
        assert_eq!(move_accuracy(100.0), 0.0);
    }
}
