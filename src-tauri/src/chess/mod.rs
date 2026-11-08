//! Chess rules live in `shakmaty`. This module is the thin, validated layer OUTPOST
//! uses everywhere a position crosses a trust boundary: before a FEN reaches an
//! engine (Stockfish 19 terminates on invalid input), before an AI prompt is built,
//! and before a move suggested by anything other than the board is played.

pub mod pgn;

use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use shakmaty::fen::Fen;
use shakmaty::san::SanPlus;
use shakmaty::uci::UciMove;
use shakmaty::{Bitboard, CastlingMode, Chess, Color, EnPassantMode, Move, Position, Role, Square};

pub const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// Parses and fully validates a FEN for standard chess.
pub fn parse_fen(fen: &str) -> Result<Chess> {
    let fen = fen.trim();
    if fen.is_empty() || fen.len() > 120 || fen.chars().any(|c| c.is_control()) {
        bail!("Not a FEN string.");
    }
    let parsed = Fen::from_ascii(fen.as_bytes()).map_err(|e| anyhow!("Invalid FEN: {e}."))?;
    parsed
        .into_position::<Chess>(CastlingMode::Standard)
        .map_err(|e| anyhow!("Illegal position: {e}."))
}

pub fn to_fen(pos: &Chess) -> String {
    Fen::from_position(pos, EnPassantMode::Legal).to_string()
}

/// FEN without move counters: identifies a position for caching and repetition.
pub fn position_key(pos: &Chess) -> String {
    let fen = to_fen(pos);
    fen.split(' ').take(4).collect::<Vec<_>>().join(" ")
}

pub fn parse_uci(pos: &Chess, uci: &str) -> Result<Move> {
    let parsed: UciMove = uci.trim().parse().map_err(|_| anyhow!("'{uci}' is not a UCI move."))?;
    parsed.to_move(pos).map_err(|_| anyhow!("'{uci}' is not legal in this position."))
}

/// UCI with standard castling notation (e1g1), which is what UCI engines expect
/// when `UCI_Chess960` is off.
pub fn uci_string(m: Move) -> String {
    UciMove::from_move(m, CastlingMode::Standard).to_string()
}

pub fn san_string(pos: &Chess, m: Move) -> String {
    SanPlus::from_move(pos.clone(), m).to_string()
}

pub fn parse_san(pos: &Chess, san: &str) -> Option<Move> {
    let san: SanPlus = san.trim().parse().ok()?;
    san.san.to_move(pos).ok()
}

/// Replays a move list from a starting FEN, rejecting the whole request on the
/// first illegal move. Returns every position (index 0 = start).
pub fn replay(start_fen: Option<&str>, moves: &[String]) -> Result<Vec<Chess>> {
    let start = match start_fen {
        Some(fen) if !fen.trim().is_empty() => parse_fen(fen)?,
        _ => Chess::default(),
    };
    let mut positions = Vec::with_capacity(moves.len() + 1);
    positions.push(start);
    for (index, uci) in moves.iter().enumerate() {
        let pos = positions.last().unwrap();
        let m = parse_uci(pos, uci).map_err(|e| anyhow!("Move {}: {e}", index + 1))?;
        let next = pos.clone().play(m).map_err(|_| anyhow!("Move {} is illegal.", index + 1))?;
        positions.push(next);
    }
    Ok(positions)
}

/// Converts a UCI principal variation into SAN, truncating at the first move that
/// is not legal (some engines append junk to PVs; nothing illegal is ever shown).
pub fn pv_to_san(pos: &Chess, pv: &[String]) -> (Vec<String>, Vec<String>) {
    let mut pos = pos.clone();
    let mut san = Vec::with_capacity(pv.len());
    let mut uci = Vec::with_capacity(pv.len());
    for mv in pv {
        let Ok(m) = parse_uci(&pos, mv) else { break };
        san.push(san_string(&pos, m));
        uci.push(uci_string(m));
        pos.play_unchecked(m);
    }
    (san, uci)
}

pub fn piece_value(role: Role) -> i32 {
    match role {
        Role::Pawn => 1,
        Role::Knight | Role::Bishop => 3,
        Role::Rook => 5,
        Role::Queen => 9,
        Role::King => 100,
    }
}

/// Material balance in pawns, from White's point of view.
pub fn material_balance(pos: &Chess) -> i32 {
    let board = pos.board();
    let mut total = 0;
    for sq in board.occupied() {
        if let Some(piece) = board.piece_at(sq) {
            if piece.role != Role::King {
                let v = piece_value(piece.role);
                total += if piece.color == Color::White { v } else { -v };
            }
        }
    }
    total
}

fn least_valuable(pos: &Chess, attackers: Bitboard) -> Option<(Square, Role)> {
    let board = pos.board();
    [Role::Pawn, Role::Knight, Role::Bishop, Role::Rook, Role::Queen, Role::King]
        .into_iter()
        .find_map(|role| (attackers & board.by_role(role)).first().map(|sq| (sq, role)))
}

/// Static exchange evaluation: material (in pawns) that `side` can expect to win by
/// starting a capture sequence on `target`, with both sides allowed to stop. Uses the
/// standard swap algorithm with x-ray attackers revealed as pieces leave the board.
/// Pins and checks are ignored, as is usual for SEE.
pub fn see(pos: &Chess, target: Square, side: Color) -> i32 {
    let board = pos.board();
    let Some(victim) = board.role_at(target) else { return 0 };
    let mut occupied = board.occupied();
    let mut gains = vec![piece_value(victim)];
    let mut turn = side;
    let mut on_square = None;
    loop {
        let attackers = board.attacks_to(target, turn, occupied) & occupied;
        let Some((from, role)) = least_valuable(pos, attackers) else { break };
        if let Some(prev) = on_square {
            gains.push(piece_value(prev) - gains.last().copied().unwrap_or(0));
        }
        on_square = Some(role);
        occupied.discard(from);
        turn = !turn;
        // A king may only capture if the square is not defended afterwards.
        if role == Role::King && (board.attacks_to(target, turn, occupied) & occupied).any() {
            gains.pop();
            break;
        }
    }
    if on_square.is_none() || gains.is_empty() {
        return 0;
    }
    while gains.len() > 1 {
        let last = gains.pop().unwrap();
        let prev = gains.last_mut().unwrap();
        *prev = -std::cmp::max(-*prev, last);
    }
    gains[0].max(0)
}

#[derive(Clone, Debug, Serialize)]
pub struct HangingPiece {
    pub square: String,
    pub role: String,
    pub color: String,
    /// Pawns the opponent wins by exchanging on this square (static estimate).
    pub exchange_gain: i32,
}

/// Pieces that the side NOT owning them can win material against right now,
/// according to static exchange evaluation. Facts for AI context, not engine output.
pub fn en_prise(pos: &Chess) -> Vec<HangingPiece> {
    let board = pos.board();
    let mut out = Vec::new();
    for sq in board.occupied() {
        let Some(piece) = board.piece_at(sq) else { continue };
        if piece.role == Role::King {
            continue;
        }
        let gain = see(pos, sq, !piece.color);
        if gain > 0 {
            out.push(HangingPiece {
                square: sq.to_string(),
                role: role_name(piece.role).into(),
                color: color_name(piece.color).into(),
                exchange_gain: gain,
            });
        }
    }
    out
}

pub fn role_name(role: Role) -> &'static str {
    match role {
        Role::Pawn => "pawn",
        Role::Knight => "knight",
        Role::Bishop => "bishop",
        Role::Rook => "rook",
        Role::Queen => "queen",
        Role::King => "king",
    }
}

pub fn color_name(color: Color) -> &'static str {
    match color {
        Color::White => "white",
        Color::Black => "black",
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GameStatus {
    pub over: bool,
    pub check: bool,
    /// "checkmate" | "stalemate" | "insufficient" | null
    pub reason: Option<String>,
    /// "1-0" | "0-1" | "1/2-1/2" | null
    pub result: Option<String>,
    pub fifty_move_claimable: bool,
}

pub fn status(pos: &Chess) -> GameStatus {
    let check = pos.is_check();
    let (over, reason, result) = if pos.is_checkmate() {
        let result = if pos.turn() == Color::White { "0-1" } else { "1-0" };
        (true, Some("checkmate"), Some(result))
    } else if pos.is_stalemate() {
        (true, Some("stalemate"), Some("1/2-1/2"))
    } else if pos.is_insufficient_material() {
        (true, Some("insufficient"), Some("1/2-1/2"))
    } else {
        (false, None, None)
    };
    GameStatus {
        over,
        check,
        reason: reason.map(str::to_string),
        result: result.map(str::to_string),
        fifty_move_claimable: pos.halfmoves() >= 100,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moves(list: &str) -> Vec<String> {
        list.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn rejects_garbage_and_illegal_fens() {
        assert!(parse_fen("").is_err());
        assert!(parse_fen("hello").is_err());
        assert!(parse_fen("8/8/8/8/8/8/8/8 w - - 0 1").is_err(), "no kings");
        assert!(parse_fen("4k3/8/8/8/8/8/8/4K2R w - - 0 1\nquit").is_err(), "control characters");
        // Side not to move is in check: illegal.
        assert!(parse_fen("4k3/4Q3/8/8/8/8/8/4K3 w - - 0 1").is_err());
        assert!(parse_fen(START_FEN).is_ok());
    }

    #[test]
    fn castling_is_standard_uci() {
        let positions = replay(None, &moves("e2e4 e7e5 g1f3 b8c6 f1c4 g8f6 e1g1")).unwrap();
        let last = positions.last().unwrap();
        assert!(to_fen(last).starts_with("r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQ1RK1 b kq"));
        let m = parse_uci(&positions[6], "e1g1").unwrap();
        assert_eq!(uci_string(m), "e1g1");
        assert_eq!(san_string(&positions[6], m), "O-O");
    }

    #[test]
    fn en_passant_and_promotion() {
        let positions = replay(None, &moves("e2e4 a7a6 e4e5 d7d5 e5d6")).unwrap();
        let m = parse_uci(&positions[4], "e5d6").unwrap();
        assert!(matches!(m, Move::EnPassant { .. }));
        assert_eq!(san_string(&positions[4], m), "exd6");

        let pos = parse_fen("8/P6k/8/8/8/8/8/K7 w - - 0 1").unwrap();
        for (uci, san) in [("a7a8q", "a8=Q"), ("a7a8n", "a8=N"), ("a7a8r", "a8=R"), ("a7a8b", "a8=B")] {
            let m = parse_uci(&pos, uci).unwrap();
            assert_eq!(san_string(&pos, m), san);
        }
        assert!(parse_uci(&pos, "a7a8").is_err(), "promotion piece is required");
    }

    #[test]
    fn replay_rejects_illegal_move_with_index() {
        let err = replay(None, &moves("e2e4 e7e5 e1e3")).unwrap_err();
        assert!(err.to_string().contains("Move 3"), "{err}");
    }

    #[test]
    fn pv_truncates_at_illegal_move() {
        let pos = Chess::default();
        let (san, uci) = pv_to_san(&pos, &moves("e2e4 e7e5 g1f3 a1a8 b8c6"));
        assert_eq!(san, vec!["e4", "e5", "Nf3"]);
        assert_eq!(uci.len(), 3);
    }

    #[test]
    fn status_detects_mate_stalemate_and_insufficient() {
        let mate = replay(None, &moves("f2f3 e7e5 g2g4 d8h4")).unwrap().pop().unwrap();
        let s = status(&mate);
        assert!(s.over && s.check);
        assert_eq!(s.reason.as_deref(), Some("checkmate"));
        assert_eq!(s.result.as_deref(), Some("0-1"));

        let stalemate = parse_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
        assert_eq!(status(&stalemate).reason.as_deref(), Some("stalemate"));

        let kk = parse_fen("8/8/4k3/8/8/3K4/8/8 w - - 0 1").unwrap();
        assert_eq!(status(&kk).reason.as_deref(), Some("insufficient"));

        let fifty = parse_fen("8/8/4k3/8/8/3K4/8/7R w - - 100 80").unwrap();
        assert!(status(&fifty).fifty_move_claimable);
    }

    #[test]
    fn static_exchange_evaluation() {
        // Undefended knight on e5 attacked by a pawn on d4: white wins 3.
        let pos = parse_fen("4k3/8/8/4n3/3P4/8/8/4K3 w - - 0 1").unwrap();
        assert_eq!(see(&pos, Square::E5, Color::White), 3);
        // Pawn defended by pawn, attacked by queen: capturing loses the queen, so 0.
        let pos = parse_fen("4k3/8/3p4/4p3/8/8/7Q/4K3 w - - 0 1").unwrap();
        assert_eq!(see(&pos, Square::E5, Color::White), 0);
        // Rook defended by pawn, attacked by knight: N takes R (5), pawn takes N (3) => +2.
        let pos = parse_fen("4k3/8/3p4/4r3/8/3N4/8/4K3 w - - 0 1").unwrap();
        assert_eq!(see(&pos, Square::E5, Color::White), 2);
        // X-ray: two white rooks on the e-file against a defended pawn.
        let pos = parse_fen("4k3/4r3/8/4p3/8/8/4R3/4R1K1 w - - 0 1").unwrap();
        assert_eq!(see(&pos, Square::E5, Color::White), 1);
    }

    #[test]
    fn en_prise_lists_loose_pieces() {
        let pos = parse_fen("4k3/8/8/4n3/3P4/8/8/4K3 b - - 0 1").unwrap();
        let hanging = en_prise(&pos);
        assert_eq!(hanging.len(), 1);
        assert_eq!(hanging[0].square, "e5");
        assert_eq!(hanging[0].role, "knight");
    }
}
