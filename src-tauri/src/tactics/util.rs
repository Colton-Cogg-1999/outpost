// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Board helpers for the tactic generator and tagger. Ported from lichess-puzzler
// (https://github.com/ornicar/lichess-puzzler, tagger/util.py and generator/util.py,
// AGPL-3.0, by Thibault Duplessis and contributors) to Rust and shakmaty.

use shakmaty::attacks::{between, ray};
use shakmaty::{Bitboard, Board, Chess, Color, Move, Piece, Position, Role, Square};

/// Material values used by the tagger (the king has no material value).
pub fn value(role: Role) -> i32 {
    match role {
        Role::Pawn => 1,
        Role::Knight | Role::Bishop => 3,
        Role::Rook => 5,
        Role::Queen => 9,
        Role::King => 0,
    }
}

/// Like `value`, but the king outranks everything (`king_values` upstream).
pub fn king_value(role: Role) -> i32 {
    if role == Role::King { 99 } else { value(role) }
}

/// python-chess piece type numbers, needed where upstream compares a value with a type.
pub fn type_number(role: Role) -> i32 {
    match role {
        Role::Pawn => 1,
        Role::Knight => 2,
        Role::Bishop => 3,
        Role::Rook => 4,
        Role::Queen => 5,
        Role::King => 6,
    }
}

pub fn is_ray_piece(role: Role) -> bool {
    matches!(role, Role::Queen | Role::Rook | Role::Bishop)
}

pub fn material_count(board: &Board, side: Color) -> i32 {
    [Role::Pawn, Role::Knight, Role::Bishop, Role::Rook, Role::Queen]
        .into_iter()
        .map(|r| board.by_piece(Piece { color: side, role: r }).count() as i32 * value(r))
        .sum()
}

pub fn material_diff(board: &Board, side: Color) -> i32 {
    material_count(board, side) - material_count(board, side.other())
}

pub fn is_up_in_material(board: &Board, side: Color) -> bool {
    material_diff(board, side) > 0
}

/// Destination square in standard (king-moves-two) notation, as python-chess reports it.
pub fn to_sq(m: Move, mover: Color) -> Square {
    match m.castling_side() {
        Some(side) => side.king_to(mover),
        None => m.to(),
    }
}

pub fn from_sq(m: Move) -> Square {
    m.from().unwrap_or(m.to())
}

pub fn attackers(board: &Board, color: Color, sq: Square) -> Bitboard {
    board.attacks_to(sq, color, board.occupied())
}

/// The pieces of the opponent of `pov` attacked by the piece standing on `from`.
pub fn attacked_opponent_squares(board: &Board, from: Square, pov: Color) -> Vec<(Piece, Square)> {
    board
        .attacks_from(from)
        .into_iter()
        .filter_map(|sq| board.piece_at(sq).filter(|p| p.color != pov).map(|p| (p, sq)))
        .collect()
}

pub fn is_defended(board: &Board, piece: Piece, sq: Square) -> bool {
    if attackers(board, piece.color, sq).any() {
        return true;
    }
    // Ray defence: a defender behind an attacking slider (x-ray).
    for attacker in attackers(board, piece.color.other(), sq) {
        if board.role_at(attacker).is_some_and(is_ray_piece) {
            let mut copy = board.clone();
            copy.discard_piece_at(attacker);
            if attackers(&copy, piece.color, sq).any() {
                return true;
            }
        }
    }
    false
}

pub fn is_hanging(board: &Board, piece: Piece, sq: Square) -> bool {
    !is_defended(board, piece, sq)
}

pub fn can_be_taken_by_lower_piece(board: &Board, piece: Piece, sq: Square) -> bool {
    attackers(board, piece.color.other(), sq)
        .into_iter()
        .filter_map(|a| board.role_at(a))
        .any(|r| r != Role::King && value(r) < king_value(piece.role))
}

/// Hanging, or takeable by a lower piece.
pub fn is_in_bad_spot(board: &Board, sq: Square) -> bool {
    let Some(piece) = board.piece_at(sq) else { return false };
    attackers(board, piece.color.other(), sq).any() && (is_hanging(board, piece, sq) || can_be_taken_by_lower_piece(board, piece, sq))
}

/// python-chess `Board.pin`: the line through the king and the pinner, or None.
pub fn pin(board: &Board, color: Color, sq: Square) -> Option<Bitboard> {
    let king = board.king_of(color)?;
    if king == sq {
        return None;
    }
    let orthogonal = king.file() == sq.file() || king.rank() == sq.rank();
    let diagonal = !orthogonal && ray(king, sq).any();
    if !orthogonal && !diagonal {
        return None;
    }
    let sliders = if orthogonal { board.rooks() | board.queens() } else { board.bishops() | board.queens() };
    let line = ray(king, sq);
    for sniper in sliders & board.by_color(color.other()) & line {
        // The sniper must be on the far side of `sq`, with nothing else in between.
        if between(sniper, king).contains(sq) && (between(sniper, king) & board.occupied()) == Bitboard::from_square(sq) {
            return Some(ray(king, sniper));
        }
    }
    None
}

pub fn is_pinned(board: &Board, color: Color, sq: Square) -> bool {
    pin(board, color, sq).is_some()
}

/// Squares a piece could move to ignoring checks and pins (python-chess pseudo-legal moves).
pub fn pseudo_destinations(pos: &Chess, from: Square) -> Bitboard {
    let board = pos.board();
    let Some(piece) = board.piece_at(from) else { return Bitboard::EMPTY };
    let own = board.by_color(piece.color);
    if piece.role != Role::Pawn {
        return board.attacks_from(from) & !own;
    }
    let theirs = board.by_color(piece.color.other());
    let mut out = board.attacks_from(from) & theirs;
    let dir = if piece.color == Color::White { 8 } else { -8 };
    if let Some(one) = from.offset(dir).filter(|s| !board.occupied().contains(*s)) {
        out = out.with(one);
        let start = if piece.color == Color::White { shakmaty::Rank::Second } else { shakmaty::Rank::Seventh };
        if from.rank() == start {
            if let Some(two) = one.offset(dir).filter(|s| !board.occupied().contains(*s)) {
                out = out.with(two);
            }
        }
    }
    out
}

/// Escapes for a piece in a bad spot. Port of `is_trapped`.
pub fn is_trapped(pos: &Chess, sq: Square) -> bool {
    let board = pos.board();
    if pos.is_check() || is_pinned(board, pos.turn(), sq) {
        return false;
    }
    let Some(piece) = board.piece_at(sq) else { return false };
    if matches!(piece.role, Role::Pawn | Role::King) {
        return false;
    }
    if !is_in_bad_spot(board, sq) {
        return false;
    }
    for escape in pos.legal_moves() {
        if escape.from() != Some(sq) {
            continue;
        }
        let to = to_sq(escape, pos.turn());
        if let Some(capturing) = board.piece_at(to) {
            if value(capturing.role) >= value(piece.role) {
                return false;
            }
        }
        let mut after = pos.clone();
        after.play_unchecked(escape);
        if !is_in_bad_spot(after.board(), to) {
            return false;
        }
    }
    true
}

pub fn squares_are_collinear(a: Square, b: Square, c: Square) -> bool {
    let (r1, f1) = (a.rank().to_u32() as i32, a.file().to_u32() as i32);
    let (r2, f2) = (b.rank().to_u32() as i32, b.file().to_u32() as i32);
    let (r3, f3) = (c.rank().to_u32() as i32, c.file().to_u32() as i32);
    (r1 == r2 && r2 == r3)
        || (f1 == f2 && f2 == f3)
        || (r1 - f1 == r2 - f2 && r2 - f2 == r3 - f3)
        || (r1 + f1 == r2 + f2 && r2 + f2 == r3 + f3)
}

pub fn count_mates(pos: &Chess) -> usize {
    pos.legal_moves()
        .into_iter()
        .filter(|m| {
            let mut after = pos.clone();
            after.play_unchecked(*m);
            after.is_checkmate()
        })
        .count()
}

/// Castling rights a position would have if nobody had given any up: python-chess
/// `maximum_castling_rights` (rooks on their corners while the king is on e1/e8).
pub fn maximum_castling_rights(board: &Board) -> Bitboard {
    let mut out = Bitboard::EMPTY;
    if board.king_of(Color::White) == Some(Square::E1) {
        out |= board.by_piece(Piece { color: Color::White, role: Role::Rook }) & (Bitboard::from_square(Square::A1) | Bitboard::from_square(Square::H1));
    }
    if board.king_of(Color::Black) == Some(Square::E8) {
        out |= board.by_piece(Piece { color: Color::Black, role: Role::Rook }) & (Bitboard::from_square(Square::A8) | Bitboard::from_square(Square::H8));
    }
    out
}

/// A puzzle line: `positions[0]` is the position before the opponent's mistake,
/// `moves[0]` is that mistake, and the solver's moves are `moves[1]`, `moves[3]`, …
/// "Node i" (as in python-chess) is the position after `moves[i]`.
pub struct Line {
    pub positions: Vec<Chess>,
    pub moves: Vec<Move>,
    /// Upstream evaluation of the final position, solver's point of view (mate = 999999999).
    pub cp: i64,
}

impl Line {
    pub fn new(start: Chess, moves: &[Move], cp: i64) -> Option<Line> {
        let mut positions = vec![start];
        for m in moves {
            let next = positions.last()?.clone().play(*m).ok()?;
            positions.push(next);
        }
        Some(Line { positions, moves: moves.to_vec(), cp })
    }

    /// The solver: the side to move after the mistake.
    pub fn pov(&self) -> Color {
        self.positions[0].turn().other()
    }
    pub fn len(&self) -> usize {
        self.moves.len()
    }
    /// Board after node i (python `mainline[i].board()`).
    pub fn after(&self, i: usize) -> &Chess {
        &self.positions[i + 1]
    }
    /// Board before node i (python `mainline[i].parent.board()`).
    pub fn before(&self, i: usize) -> &Chess {
        &self.positions[i]
    }
    pub fn mover(&self, i: usize) -> Color {
        self.positions[i].turn()
    }
    pub fn to(&self, i: usize) -> Square {
        to_sq(self.moves[i], self.mover(i))
    }
    pub fn from(&self, i: usize) -> Square {
        from_sq(self.moves[i])
    }
    /// Piece type standing on the destination after the move.
    pub fn moved_role(&self, i: usize) -> Role {
        self.after(i).board().role_at(self.to(i)).unwrap_or(self.moves[i].role())
    }
    pub fn is_capture(&self, i: usize) -> bool {
        self.moves[i].is_capture()
    }
    pub fn is_castling(&self, i: usize) -> bool {
        self.moves[i].is_castle()
    }
    /// Solver nodes: 1, 3, 5, …
    pub fn solver_nodes(&self) -> impl Iterator<Item = usize> + '_ {
        (1..self.len()).step_by(2)
    }
    pub fn last(&self) -> usize {
        self.len() - 1
    }
    pub fn end(&self) -> &Chess {
        self.positions.last().expect("a line has a start position")
    }

    pub fn is_advanced_pawn_move(&self, i: usize) -> bool {
        if self.moves[i].is_promotion() {
            return true;
        }
        if self.moved_role(i) != Role::Pawn {
            return false;
        }
        let rank = self.to(i).rank().to_u32();
        if self.mover(i) == Color::Black { rank < 3 } else { rank > 4 }
    }

    pub fn is_very_advanced_pawn_move(&self, i: usize) -> bool {
        if !self.is_advanced_pawn_move(i) {
            return false;
        }
        let rank = self.to(i).rank().to_u32();
        if self.mover(i) == Color::Black { rank < 2 } else { rank > 5 }
    }
}
