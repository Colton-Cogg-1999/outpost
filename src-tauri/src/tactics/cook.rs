// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Theme tagger. A port of lichess-puzzler's tagger/cook.py
// (https://github.com/ornicar/lichess-puzzler, AGPL-3.0, by Thibault Duplessis and
// contributors) to Rust and shakmaty. Theme names match the Lichess puzzle themes.
// Python indexing is kept in comments: `mainline[i]` is node i of `Line`.

use super::util::*;
use shakmaty::attacks::between;
use shakmaty::{Bitboard, Color, File, Piece, Position, Rank, Role, Square};

pub fn cook(p: &Line) -> Vec<&'static str> {
    let mut tags: Vec<&'static str> = Vec::new();
    if let Some(mate) = mate_in(p) {
        tags.push(mate);
        tags.push("mate");
        if smothered_mate(p) {
            tags.push("smotheredMate");
        } else if back_rank_mate(p) {
            tags.push("backRankMate");
        } else if anastasia_mate(p) {
            tags.push("anastasiaMate");
        } else if hook_mate(p) {
            tags.push("hookMate");
        } else if arabian_mate(p) {
            tags.push("arabianMate");
        } else if let Some(found) = boden_or_double_bishop_mate(p) {
            tags.push(found);
        } else if dovetail_mate(p) {
            tags.push("dovetailMate");
        }
    } else if p.cp > 600 {
        tags.push("crushing");
    } else if p.cp > 200 {
        tags.push("advantage");
    } else {
        tags.push("equality");
    }
    if attraction(p) {
        tags.push("attraction");
    }
    if deflection(p) {
        tags.push("deflection");
    }
    if advanced_pawn(p) {
        tags.push("advancedPawn");
    }
    if double_check(p) {
        tags.push("doubleCheck");
    }
    if quiet_move(p) {
        tags.push("quietMove");
    }
    if defensive_move(p) || check_escape(p) {
        tags.push("defensiveMove");
    }
    if sacrifice(p) {
        tags.push("sacrifice");
    }
    if x_ray(p) {
        tags.push("xRayAttack");
    }
    if fork(p) {
        tags.push("fork");
    }
    if hanging_piece(p) {
        tags.push("hangingPiece");
    }
    if trapped_piece(p) {
        tags.push("trappedPiece");
    }
    if discovered_attack(p) {
        tags.push("discoveredAttack");
    }
    if exposed_king(p) {
        tags.push("exposedKing");
    }
    if skewer(p) {
        tags.push("skewer");
    }
    if collinear(p) {
        tags.push("collinearMove");
    }
    if self_interference(p) || interference(p) {
        tags.push("interference");
    }
    if intermezzo(p) {
        tags.push("intermezzo");
    }
    if pin_prevents_attack(p) || pin_prevents_escape(p) {
        tags.push("pin");
    }
    if attacking_f2_f7(p) {
        tags.push("attackingF2F7");
    }
    if clearance(p) {
        tags.push("clearance");
    }
    if en_passant(p) {
        tags.push("enPassant");
    }
    if castling(p) {
        tags.push("castling");
    }
    if promotion(p) {
        tags.push("promotion");
    }
    if under_promotion(p) {
        tags.push("underPromotion");
    }
    if capturing_defender(p) {
        tags.push("capturingDefender");
    }
    if piece_endgame(p, Role::Pawn) {
        tags.push("pawnEndgame");
    } else if piece_endgame(p, Role::Queen) {
        tags.push("queenEndgame");
    } else if piece_endgame(p, Role::Rook) {
        tags.push("rookEndgame");
    } else if piece_endgame(p, Role::Bishop) {
        tags.push("bishopEndgame");
    } else if piece_endgame(p, Role::Knight) {
        tags.push("knightEndgame");
    } else if queen_rook_endgame(p) {
        tags.push("queenRookEndgame");
    }
    if !tags.contains(&"backRankMate") && !tags.contains(&"fork") {
        if kingside_attack(p) {
            tags.push("kingsideAttack");
        } else if queenside_attack(p) {
            tags.push("queensideAttack");
        }
    }
    tags.push(match p.len() {
        2 => "oneMove",
        4 => "short",
        n if n >= 8 => "veryLong",
        _ => "long",
    });
    tags
}

fn bb(sq: Square) -> Bitboard {
    Bitboard::from_square(sq)
}

pub fn advanced_pawn(p: &Line) -> bool {
    p.solver_nodes().any(|i| p.is_very_advanced_pawn_move(i))
}

pub fn double_check(p: &Line) -> bool {
    p.solver_nodes().any(|i| p.after(i).checkers().more_than_one())
}

pub fn sacrifice(p: &Line) -> bool {
    // Down in material compared to the initial position, after moving.
    let pov = p.pov();
    let diffs: Vec<i32> = (0..p.len()).map(|i| material_diff(p.after(i).board(), pov)).collect();
    let initial = diffs[0];
    for i in (1..p.len()).step_by(2).skip(1) {
        if diffs[i] - initial <= -2 {
            return !(2..p.len()).step_by(2).any(|j| p.moves[j].is_promotion());
        }
    }
    false
}

pub fn x_ray(p: &Line) -> bool {
    for i in p.solver_nodes().skip(1) {
        if !p.is_capture(i) {
            continue;
        }
        let op = i - 1;
        if p.to(op) != p.to(i) || p.moved_role(op) == Role::King {
            continue;
        }
        let pl = i - 2;
        if p.to(pl) != p.to(op) {
            continue;
        }
        if between(p.from(i), p.to(i)).contains(p.from(op)) {
            return true;
        }
    }
    false
}

pub fn fork(p: &Line) -> bool {
    let pov = p.pov();
    let nodes: Vec<usize> = p.solver_nodes().collect();
    for &i in nodes.iter().take(nodes.len().saturating_sub(1)) {
        let board = p.after(i).board();
        let to = p.to(i);
        if is_in_bad_spot(board, to) {
            continue;
        }
        let mut nb = 0;
        for (piece, sq) in attacked_opponent_squares(board, to, pov) {
            if piece.role == Role::Pawn {
                continue;
            }
            if king_value(piece.role) > king_value(p.moved_role(i))
                || (is_hanging(board, piece, sq) && !attackers(board, pov.other(), to).contains(sq))
            {
                nb += 1;
            }
        }
        if nb > 1 {
            return true;
        }
    }
    false
}

pub fn hanging_piece(p: &Line) -> bool {
    let to = p.to(1);
    let first = p.after(0);
    let captured = first.board().piece_at(to);
    if first.is_check() && captured.is_none_or(|c| c.role == Role::Pawn) {
        return false;
    }
    if let Some(captured) = captured.filter(|c| c.role != Role::Pawn) {
        if is_hanging(first.board(), captured, to) {
            let op_to = p.to(0);
            let op_capture = p.before(0).board().piece_at(op_to);
            if op_capture.is_some_and(|c| value(c.role) >= value(captured.role)) && op_to == to {
                return false;
            }
            if p.len() < 4 {
                return true;
            }
            if material_diff(p.after(3).board(), p.pov()) >= material_diff(p.after(1).board(), p.pov()) {
                return true;
            }
        }
    }
    false
}

pub fn trapped_piece(p: &Line) -> bool {
    for i in p.solver_nodes().skip(1) {
        let mut square = p.to(i);
        let captured = p.before(i).board().piece_at(square);
        if captured.is_some_and(|c| c.role != Role::Pawn) {
            let prev = i - 1;
            if p.to(prev) == square {
                square = p.from(prev);
            }
            if is_trapped(p.before(prev), square) {
                return true;
            }
        }
    }
    false
}

pub fn discovered_attack(p: &Line) -> bool {
    if discovered_check(p) {
        return true;
    }
    for i in p.solver_nodes().skip(1) {
        if p.is_capture(i) {
            let line = between(p.from(i), p.to(i));
            if p.to(i - 1) == p.to(i) {
                return false;
            }
            let prev = i - 2;
            if line.contains(p.from(prev)) && p.to(i) != p.to(prev) && p.from(i) != p.to(prev) && !p.is_castling(prev) {
                return true;
            }
        }
    }
    false
}

pub fn discovered_check(p: &Line) -> bool {
    p.solver_nodes().any(|i| {
        let checkers = p.after(i).checkers();
        checkers.any() && !checkers.contains(p.to(i))
    })
}

pub fn quiet_move(p: &Line) -> bool {
    let pov = p.pov();
    p.solver_nodes().any(|i| {
        i != p.last()
            && !p.after(i).is_check()
            && !p.before(i).is_check()
            && !p.is_capture(i)
            && attacked_opponent_squares(p.after(i).board(), p.to(i), pov).is_empty()
            && !p.is_advanced_pawn_move(i)
            && p.moved_role(i) != Role::King
    })
}

pub fn defensive_move(p: &Line) -> bool {
    // Like quiet_move, but on the last move, with at least 3 legal moves.
    if p.len() < 2 || p.before(p.last()).legal_moves().len() < 3 {
        return false;
    }
    let i = p.last();
    if p.after(i).is_check() || p.is_capture(i) {
        return false;
    }
    if !attacked_opponent_squares(p.after(i).board(), p.to(i), p.pov()).is_empty() {
        return false;
    }
    !p.is_advanced_pawn_move(i)
}

pub fn check_escape(p: &Line) -> bool {
    for i in p.solver_nodes() {
        if p.after(i).is_check() || p.is_capture(i) {
            return false;
        }
        if p.before(i).legal_moves().len() < 3 {
            return false;
        }
        if p.before(i).is_check() {
            return true;
        }
    }
    false
}

pub fn attraction(p: &Line) -> bool {
    let pov = p.pov();
    for i in p.solver_nodes() {
        // 1. player moves to a square
        let first_to = p.to(i);
        // 2. opponent captures on that square
        let reply = i + 1;
        if reply < p.len() && p.to(reply) == first_to {
            let attracted = p.moved_role(reply);
            if matches!(attracted, Role::King | Role::Queen | Role::Rook) {
                let attracted_to = p.to(reply);
                let next = reply + 1;
                if next < p.len() {
                    // 3. player attacks that square
                    let attacking = attackers(p.after(next).board(), pov, attracted_to);
                    if attacking.contains(p.to(next)) {
                        // 4. player checks on that square
                        if attracted == Role::King {
                            return true;
                        }
                        // 4. or player later captures on that square
                        let n3 = next + 2;
                        if n3 < p.len() && p.to(n3) == attracted_to {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

pub fn deflection(p: &Line) -> bool {
    for i in p.solver_nodes().skip(1) {
        let captured = p.before(i).board().piece_at(p.to(i));
        let promo = p.moves[i].is_promotion();
        if captured.is_none() && !promo {
            continue;
        }
        let capturing = p.moved_role(i);
        if captured.is_some_and(|c| king_value(c.role) > king_value(capturing)) {
            continue;
        }
        let square = p.to(i);
        let op = i - 1;
        let grandpa = i - 2;
        let prev_player_capture = p.before(grandpa).board().piece_at(p.to(grandpa));
        let op_from_attacks = p.after(grandpa).board().attacks_from(p.from(op));
        // Upstream compares a material value with a python-chess piece type number here.
        if prev_player_capture.is_none_or(|c| value(c.role) < type_number(p.moved_role(grandpa)))
            && square != p.to(op)
            && square != p.to(grandpa)
            && (p.to(op) == p.to(grandpa) || p.after(grandpa).is_check())
            && (op_from_attacks.contains(square)
                || (promo && p.to(i).file() == p.from(op).file() && op_from_attacks.contains(p.from(i))))
            && !p.after(op).board().attacks_from(p.to(op)).contains(square)
        {
            return true;
        }
    }
    false
}

pub fn exposed_king(p: &Line) -> bool {
    let pov = p.pov();
    let board = p.after(0).board();
    let Some(king) = board.king_of(pov.other()) else { return false };
    // Upstream mirrors the board for Black; here the directions flip instead.
    let (rank, forward) = if pov == Color::White { (king.rank().to_u32(), -1i32) } else { (7 - king.rank().to_u32(), 1i32) };
    if rank < 5 {
        return false;
    }
    let file = king.file().to_u32();
    let mut shield: Vec<Square> = vec![];
    let at = |df: i32, dr: i32| king.offset(df + 8 * dr);
    shield.extend(at(0, forward));
    if file > 0 {
        shield.extend(at(-1, 0));
        shield.extend(at(-1, forward));
    }
    if file < 7 {
        shield.extend(at(1, 0));
        shield.extend(at(1, forward));
    }
    let pawn = Piece { color: pov.other(), role: Role::Pawn };
    if shield.iter().any(|sq| board.piece_at(*sq) == Some(pawn)) {
        return false;
    }
    let nodes: Vec<usize> = p.solver_nodes().collect();
    if nodes.len() < 3 {
        return false;
    }
    nodes[1..nodes.len() - 1].iter().any(|&i| p.after(i).is_check())
}

pub fn skewer(p: &Line) -> bool {
    for i in p.solver_nodes().skip(1) {
        let prev = i - 1;
        let capture = p.before(i).board().piece_at(p.to(i));
        if let Some(capture) = capture {
            if is_ray_piece(p.moved_role(i)) && !p.after(i).is_checkmate() {
                let line = between(p.from(i), p.to(i));
                if p.to(prev) == p.to(i) || !line.contains(p.from(prev)) {
                    continue;
                }
                if king_value(p.moved_role(prev)) > king_value(capture.role) && is_in_bad_spot(p.after(prev).board(), p.to(i)) {
                    return true;
                }
            }
        }
    }
    false
}

pub fn self_interference(p: &Line) -> bool {
    // Interference by an opponent piece.
    for i in p.solver_nodes().skip(1) {
        let prev_board = p.before(i).board();
        let square = p.to(i);
        if let Some(capture) = prev_board.piece_at(square) {
            if is_hanging(prev_board, capture, square) {
                let init = p.after(i - 2).board();
                // python-chess SquareSet.pop() takes the lowest square.
                if let Some(defender) = attackers(init, capture.color, square).first() {
                    if init.role_at(defender).is_some_and(is_ray_piece) && between(square, defender).contains(p.to(i - 1)) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

pub fn interference(p: &Line) -> bool {
    // Interference by a player piece.
    for i in p.solver_nodes().skip(1) {
        let prev_board = p.before(i).board();
        let square = p.to(i);
        if let Some(capture) = prev_board.piece_at(square) {
            if square != p.to(i - 1) && is_hanging(prev_board, capture, square) {
                let init = p.after(i - 3).board();
                // python-chess SquareSet.pop() takes the lowest square.
                if let Some(defender) = attackers(init, capture.color, square).first() {
                    if init.role_at(defender).is_some_and(is_ray_piece) && between(square, defender).contains(p.to(i - 2)) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

pub fn intermezzo(p: &Line) -> bool {
    let pov = p.pov();
    for i in p.solver_nodes().skip(1) {
        if p.is_capture(i) {
            let capture_square = p.to(i);
            let op = i - 1;
            let prev_pov = i - 2;
            if !attackers(p.after(prev_pov).board(), pov.other(), capture_square).contains(p.from(op)) && p.to(prev_pov) != capture_square {
                let prev_op = i - 3;
                return p.to(prev_op) == capture_square && p.is_capture(prev_op) && p.after(prev_op).is_legal(p.moves[i]);
            }
        }
    }
    false
}

/// The pinned piece can't attack a player piece.
pub fn pin_prevents_attack(p: &Line) -> bool {
    let pov = p.pov();
    for i in p.solver_nodes() {
        let board = p.after(i).board();
        for sq in board.by_color(pov.other()) {
            let Some(piece) = board.piece_at(sq) else { continue };
            let Some(pin_dir) = pin(board, piece.color, sq) else { continue };
            for attack in board.attacks_from(sq) {
                if let Some(attacked) = board.piece_at(attack) {
                    if attacked.color == pov
                        && !pin_dir.contains(attack)
                        && (king_value(attacked.role) > value(piece.role) || is_hanging(board, attacked, attack))
                    {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// The pinned piece can't escape the attack.
pub fn pin_prevents_escape(p: &Line) -> bool {
    let pov = p.pov();
    for i in p.solver_nodes() {
        let pos = p.after(i);
        let board = pos.board();
        for pinned_sq in board.by_color(pov.other()) {
            let Some(pinned) = board.piece_at(pinned_sq) else { continue };
            let Some(pin_dir) = pin(board, pinned.color, pinned_sq) else { continue };
            for attacker_sq in attackers(board, pov, pinned_sq) {
                if !pin_dir.contains(attacker_sq) {
                    continue;
                }
                let Some(attacker) = board.piece_at(attacker_sq) else { continue };
                if value(pinned.role) > king_value(attacker.role) {
                    return true;
                }
                if is_hanging(board, pinned, pinned_sq)
                    && !attackers(board, pov.other(), attacker_sq).contains(pinned_sq)
                    && (pseudo_destinations(pos, pinned_sq) & !pin_dir).any()
                {
                    return true;
                }
            }
        }
    }
    false
}

pub fn attacking_f2_f7(p: &Line) -> bool {
    for i in p.solver_nodes() {
        let square = p.to(i);
        if p.before(i).board().piece_at(square).is_some() && (square == Square::F2 || square == Square::F7) {
            let king = p.after(i).board().piece_at(if square == Square::F7 { Square::E8 } else { Square::E1 });
            return king.is_some_and(|k| k.role == Role::King && k.color != p.pov());
        }
    }
    false
}

pub fn kingside_attack(p: &Line) -> bool {
    side_attack(p, 7, &[6, 7], 20)
}

pub fn queenside_attack(p: &Line) -> bool {
    side_attack(p, 0, &[0, 1, 2], 18)
}

fn side_attack(p: &Line, corner_file: u32, king_files: &[u32], nb_pieces: usize) -> bool {
    let pov = p.pov();
    let back_rank = if pov == Color::White { 7 } else { 0 };
    let init = p.after(0).board();
    let Some(king) = init.king_of(pov.other()) else { return false };
    if king.rank().to_u32() != back_rank
        || !king_files.contains(&king.file().to_u32())
        || init.occupied().count() < nb_pieces
        || !p.solver_nodes().any(|i| p.after(i).is_check())
    {
        return false;
    }
    let corner = Square::from_coords(File::new(corner_file), Rank::new(back_rank));
    let mut score = 0;
    for i in p.solver_nodes() {
        let dist = corner.distance(p.to(i));
        if p.after(i).is_check() {
            score += 1;
        }
        if p.is_capture(i) && dist <= 3 {
            score += 1;
        } else if dist >= 5 {
            score -= 1;
        }
    }
    score >= 2
}

pub fn clearance(p: &Line) -> bool {
    for i in p.solver_nodes().skip(1) {
        let board = p.after(i).board();
        if p.before(i).board().piece_at(p.to(i)).is_some() {
            continue;
        }
        let Some(piece) = board.piece_at(p.to(i)) else { continue };
        if !is_ray_piece(piece.role) {
            continue;
        }
        let prev = i - 2;
        let prev_move = p.moves[prev];
        if !prev_move.is_promotion()
            && p.to(prev) != p.from(i)
            && p.to(prev) != p.to(i)
            && !p.before(i).is_check()
            && (!p.after(i).is_check() || p.moved_role(i - 1) != Role::King)
            && (p.from(prev) == p.to(i) || between(p.from(i), p.to(i)).contains(p.from(prev)))
            && (p.before(prev).board().piece_at(p.to(prev)).is_none() || is_in_bad_spot(p.after(prev).board(), p.to(prev)))
        {
            return true;
        }
    }
    false
}

pub fn en_passant(p: &Line) -> bool {
    p.solver_nodes().any(|i| p.moves[i].is_en_passant())
}

pub fn collinear(p: &Line) -> bool {
    let pov = p.pov();
    for i in p.solver_nodes() {
        // 1. player moves a ray piece without capturing
        if !is_ray_piece(p.moved_role(i)) || p.is_capture(i) {
            continue;
        }
        let prev = p.before(i);
        let (from, to) = (p.from(i), p.to(i));
        for sq in prev.board().attacks_from(from) {
            let Some(piece) = prev.board().piece_at(sq) else { continue };
            if piece.color == pov || !is_ray_piece(piece.role) {
                continue;
            }
            // 2. from, opponent piece and destination are on one line
            if !squares_are_collinear(from, sq, to) {
                continue;
            }
            // 3. the opponent piece can also move along this line
            let orthogonal = from.rank() == sq.rank() || from.file() == sq.file();
            if (orthogonal && piece.role == Role::Bishop) || (!orthogonal && piece.role == Role::Rook) {
                continue;
            }
            // 4. the capture was legal, but the player chose to stay on the line
            if prev.legal_moves().iter().any(|m| m.from() == Some(from) && m.to() == sq) {
                return true;
            }
        }
    }
    false
}

pub fn castling(p: &Line) -> bool {
    p.solver_nodes().any(|i| p.is_castling(i))
}

pub fn promotion(p: &Line) -> bool {
    p.solver_nodes().any(|i| p.moves[i].is_promotion())
}

pub fn under_promotion(p: &Line) -> bool {
    for i in p.solver_nodes() {
        if p.after(i).is_checkmate() {
            return p.moves[i].promotion() == Some(Role::Knight);
        } else if p.moves[i].promotion().is_some_and(|r| r != Role::Queen) {
            return true;
        }
    }
    false
}

pub fn capturing_defender(p: &Line) -> bool {
    for i in p.solver_nodes().skip(1) {
        let capture = p.before(i).board().piece_at(p.to(i));
        let moved = p.moved_role(i);
        if p.after(i).is_checkmate()
            || capture.is_some_and(|c| {
                moved != Role::King
                    && value(c.role) <= value(moved)
                    && is_hanging(p.before(i).board(), c, p.to(i))
                    && p.to(i - 1) != p.to(i)
            })
        {
            let prev = i - 2;
            if !p.after(prev).is_check() && p.to(prev) != p.from(i) {
                let init = p.before(prev);
                let defender_sq = p.to(prev);
                if let Some(defender) = init.board().piece_at(defender_sq) {
                    if attackers(init.board(), defender.color, p.to(i)).contains(defender_sq) && !init.is_check() {
                        return true;
                    }
                }
            }
        }
    }
    false
}

pub fn back_rank_mate(p: &Line) -> bool {
    let pov = p.pov();
    let end = p.end();
    let board = end.board();
    let Some(king) = board.king_of(pov.other()) else { return false };
    let back_rank = if pov == Color::White { 7 } else { 0 };
    if !end.is_checkmate() || king.rank().to_u32() != back_rank {
        return false;
    }
    let dir = if pov == Color::White { -8 } else { 8 };
    let mut squares: Vec<Square> = king.offset(dir).into_iter().collect();
    if king.file().to_u32() < 7 {
        squares.extend(king.offset(dir + 1));
    }
    if king.file().to_u32() > 0 {
        squares.extend(king.offset(dir - 1));
    }
    for sq in squares {
        match board.piece_at(sq) {
            None => return false,
            Some(piece) if piece.color == pov || attackers(board, pov, sq).any() => return false,
            _ => {}
        }
    }
    end.checkers().into_iter().any(|c| c.rank().to_u32() == back_rank)
}

pub fn anastasia_mate(p: &Line) -> bool {
    let pov = p.pov();
    let i = p.last();
    let board = p.end().board();
    let Some(king) = board.king_of(pov.other()) else { return false };
    let (kf, kr) = (king.file().to_u32(), king.rank().to_u32());
    if (kf == 0 || kf == 7) && kr != 0 && kr != 7 && p.to(i).file() == king.file() && matches!(p.moved_role(i), Role::Queen | Role::Rook) {
        // Upstream flips the board so the king is on the a-file; mirror the offsets instead.
        let step = if kf == 0 { 1 } else { -1 };
        let blocker = king.offset(step).and_then(|s| board.piece_at(s));
        if blocker.is_some_and(|b| b.color != pov) {
            let knight = king.offset(3 * step).and_then(|s| board.piece_at(s));
            return knight.is_some_and(|n| n.color == pov && n.role == Role::Knight);
        }
    }
    false
}

pub fn hook_mate(p: &Line) -> bool {
    let pov = p.pov();
    let i = p.last();
    let board = p.end().board();
    let Some(king) = board.king_of(pov.other()) else { return false };
    if p.moved_role(i) == Role::Rook && p.to(i).distance(king) == 1 {
        for rook_defender in attackers(board, pov, p.to(i)) {
            if board.role_at(rook_defender) == Some(Role::Knight) && rook_defender.distance(king) == 1 {
                for knight_defender in attackers(board, pov, rook_defender) {
                    if board.role_at(knight_defender) == Some(Role::Pawn) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

pub fn arabian_mate(p: &Line) -> bool {
    let pov = p.pov();
    let i = p.last();
    let board = p.end().board();
    let Some(king) = board.king_of(pov.other()) else { return false };
    let (kf, kr) = (king.file().to_u32() as i32, king.rank().to_u32() as i32);
    if (kf == 0 || kf == 7) && (kr == 0 || kr == 7) && p.moved_role(i) == Role::Rook && p.to(i).distance(king) == 1 {
        for knight_sq in attackers(board, pov, p.to(i)) {
            if board.role_at(knight_sq) == Some(Role::Knight)
                && (knight_sq.rank().to_u32() as i32 - kr).abs() == 2
                && (knight_sq.file().to_u32() as i32 - kf).abs() == 2
            {
                return true;
            }
        }
    }
    false
}

pub fn boden_or_double_bishop_mate(p: &Line) -> Option<&'static str> {
    let pov = p.pov();
    let board = p.end().board();
    let king = board.king_of(pov.other())?;
    let bishops: Vec<Square> = board.by_piece(Piece { color: pov, role: Role::Bishop }).into_iter().collect();
    if bishops.len() < 2 {
        return None;
    }
    for sq in Bitboard::FULL {
        if sq.distance(king) < 2 && !attackers(board, pov, sq).into_iter().all(|a| board.role_at(a) == Some(Role::Bishop)) {
            return None;
        }
    }
    let kf = king.file();
    if (bishops[0].file() < kf) == (bishops[1].file() > kf) { Some("bodenMate") } else { Some("doubleBishopMate") }
}

pub fn dovetail_mate(p: &Line) -> bool {
    let pov = p.pov();
    let i = p.last();
    let board = p.end().board();
    let Some(king) = board.king_of(pov.other()) else { return false };
    let (kf, kr) = (king.file().to_u32(), king.rank().to_u32());
    if kf == 0 || kf == 7 || kr == 0 || kr == 7 {
        return false;
    }
    let queen = p.to(i);
    if p.moved_role(i) != Role::Queen || queen.file() == king.file() || queen.rank() == king.rank() || queen.distance(king) > 1 {
        return false;
    }
    for sq in Bitboard::FULL {
        if sq.distance(king) != 1 || sq == queen {
            continue;
        }
        let attacking = attackers(board, pov, sq);
        if attacking == bb(queen) {
            if board.piece_at(sq).is_some() {
                return false;
            }
        } else if attacking.any() {
            return false;
        }
    }
    true
}

pub fn piece_endgame(p: &Line, role: Role) -> bool {
    [p.after(0), p.after(1)].iter().all(|pos| {
        let board = pos.board();
        board.by_role(role).any() && board.occupied().into_iter().all(|sq| board.role_at(sq).is_some_and(|r| matches!(r, Role::King | Role::Pawn) || r == role))
    })
}

pub fn queen_rook_endgame(p: &Line) -> bool {
    [p.after(0), p.after(1)].iter().all(|pos| {
        let board = pos.board();
        board.queens().count() == 1
            && board.rooks().any()
            && (board.occupied() & !(board.queens() | board.rooks() | board.pawns() | board.kings())).is_empty()
    })
}

pub fn smothered_mate(p: &Line) -> bool {
    let pov = p.pov();
    let end = p.end();
    let board = end.board();
    let Some(king) = board.king_of(pov.other()) else { return false };
    for checker in end.checkers() {
        if board.role_at(checker) == Some(Role::Knight) {
            for sq in Bitboard::FULL {
                if sq.distance(king) == 1 && board.piece_at(sq).is_none_or(|b| b.color == pov) {
                    return false;
                }
            }
            return true;
        }
    }
    false
}

pub fn mate_in(p: &Line) -> Option<&'static str> {
    if !p.end().is_checkmate() {
        return None;
    }
    Some(match p.len() / 2 {
        1 => "mateIn1",
        2 => "mateIn2",
        3 => "mateIn3",
        4 => "mateIn4",
        _ => "mateIn5",
    })
}

#[cfg(test)]
mod tests {
    //! Cases from lichess-puzzler's tagger/test.py.
    use super::*;
    use crate::chess;

    fn make(fen: &str, line: &str) -> Line {
        let pos = chess::parse_fen(fen).unwrap();
        let mut cur = pos.clone();
        let mut moves = vec![];
        for uci in line.split(' ') {
            let m = chess::parse_uci(&cur, uci).unwrap_or_else(|e| panic!("{uci} in {fen}: {e}"));
            cur = cur.play(m).unwrap();
            moves.push(m);
        }
        Line::new(pos, &moves, 999999998).unwrap()
    }

    macro_rules! case {
        ($f:ident, $want:expr, $fen:expr, $line:expr) => {
            assert_eq!($f(&make($fen, $line)), $want, "{} {} {}", stringify!($f), $fen, $line)
        };
    }

    #[test]
    fn attraction_cases() {
        case!(attraction, false, "r1bq1rk1/ppp1bppp/2n2n2/4p1B1/4N1P1/3P1N1P/PPP2P2/R2QKB1R w KQ - 1 9", "d1d2 f6e4 d3e4 c6d4 e1c1 d4f3 d2d8 e7g5 d8g5 f3g5");
        case!(attraction, false, "4r1k1/1R3ppp/1N3n2/1bP5/1P6/3p3P/6P1/3R2K1 w - - 0 28", "b6d5 f6d5 b7b5 d5c3 d1d3 c3b5");
        case!(attraction, true, "r4rk1/pp3pp1/7p/b2Pn3/4N3/6RQ/P4PPP/q1B1R1K1 b - - 8 26", "a5e1 g3g7 g8g7 h3h6 g7g8 e4f6");
        case!(attraction, true, "2kr1b1r/1p1b2pp/p1P1p2n/2P3N1/P4q2/5N2/4BKPP/R2Q3R b - - 2 18", "d7c6 d1d8 c8d8 g5e6 d8c8 e6f4");
    }

    #[test]
    fn sacrifice_cases() {
        case!(sacrifice, true, "r1b2rk1/pppp1ppp/2n5/3Q2B1/2B5/2P2N2/P1q3PP/4RK1R b - - 1 14", "d7d6 d5f7 f8f7 e1e8");
        case!(sacrifice, false, "3qr1k1/R4pbp/2p3p1/1p1p4/1P3Q2/2P1P3/3B2P1/7K b - - 0 33", "d8b8 f4f7 g8h8 f7g7");
        case!(sacrifice, true, "1R1r2k1/5ppp/p7/3q1P2/2pr1B2/3n2PP/4Q3/5RK1 b - - 4 30", "d3f4 e2e8 d8e8 b8e8");
        case!(sacrifice, true, "4r1k1/pb3ppp/1p1b1n2/2pP4/4P1q1/2N5/PBQ2PPP/R4RK1 w - - 0 19", "c2e2 d6h2 g1h2 g4h4 h2g1 f6g4 e2g4 h4g4");
        case!(sacrifice, false, "r2q1rk1/pp3p2/4pn1R/8/3Q4/5N2/PPP2PPb/R5K1 w - - 0 19", "g1h2 d8d4 f3d4 f6g4 h2g3 g4h6");
        case!(sacrifice, true, "r1bqnrk1/pp1n2p1/3bp1N1/3p1p2/2pP1P2/2P1PN1R/PP4PP/R1BQ2K1 b - - 1 15", "f8f6 h3h8 g8f7 f3g5 f7g6 d1h5");
        case!(sacrifice, true, "4r3/1b4p1/p7/1p1Pp1kr/4Qp2/1B1R1RP1/PP3P1P/2q3K1 w - - 1 31", "g1g2 h5h2 g2h2 e8h8 e4h7 h8h7 h2g2 c1h1");
        case!(sacrifice, false, "r3r1k1/pp1n1pp1/2p3p1/3p4/3PnqPN/2P4P/PPQN1P2/4RRK1 w - - 2 18", "h4g2 f4d2 c2d2 e4d2 e1e8 a8e8");
        case!(sacrifice, true, "6k1/p6p/2r2bp1/1pp4r/5P2/3R2P1/P5BP/3R3K b - - 1 29", "c5c4 d3d8 f6d8 d1d8 g8f7 g2c6");
        case!(sacrifice, false, "8/8/2R5/7P/2Pk4/p1r5/6P1/6K1 w - - 0 41", "h5h6 a3a2 c6d6 d4c5 d6d1 c3b3 h6h7 b3b1 h7h8q b1d1 g1h2 a2a1q");
    }

    #[test]
    fn defensive_and_escape_cases() {
        case!(defensive_move, false, "8/2P5/3K4/8/4pk2/2r3p1/R7/8 b - - 0 50", "f4f3 a2a3 c3a3 c7c8q");
        case!(defensive_move, false, "6k1/2Q2pp1/p5rp/3P4/2pn3r/5P1q/P1N2RPP/4R1K1 w - - 0 32", "c2d4 h4d4 c7b8 g8h7");
        case!(check_escape, true, "1R6/1P4p1/8/6k1/4K3/1r4pP/8/8 w - - 0 39", "h3h4 g5g4");
    }

    #[test]
    fn capturing_defender_cases() {
        case!(capturing_defender, true, "3rk3/1RRn4/3r1p2/3pp3/8/2P1B3/5KP1/8 b - - 3 33", "d8b8 c7d7 d6d7 b7b8");
        case!(capturing_defender, true, "2rqk2r/pp2ppbp/1n1p2p1/3P4/2P5/2N1B3/PP2QPPP/R4RK1 b k - 0 14", "c8c4 e3b6 d8b6 e2c4");
        case!(capturing_defender, false, "2r3k1/1p4bp/pq2p1p1/3pr3/4nPP1/2N4P/PPPB3K/1R1Q1R2 b - - 2 22", "e4c3 b2c3 b6b1 d1b1");
    }

    #[test]
    fn fork_cases() {
        case!(fork, true, "6q1/p6p/6p1/4k3/1P2N3/2B2P2/4K1P1/8 b - - 3 43", "e5d5 e4f6 d5c4 f6g8");
        case!(fork, false, "rnb1k2r/p1B2ppp/4p3/1Bb5/8/4P3/PP1K1PPP/nN4NR b kq - 0 12", "b8d7 b5c6 c8a6 c6a8 c5b4 b1c3");
        case!(fork, true, "r3k2r/p2q1ppp/4pn2/1Qp5/8/4P3/PP1N1PPP/R3K2R w KQkq - 2 16", "b5c5 d7d2 e1d2 f6e4 d2e2 e4c5");
        case!(fork, false, "8/p7/1p6/2p5/P6P/2P2Nk1/1r4P1/4R1K1 w - - 1 39", "f3d2 b2d2 h4h5 d2g2");
        case!(fork, false, "rnbq1b1r/p1k1pQp1/2p4p/1p1nP1p1/2pP4/2N3B1/PP3P1P/R3KBNR w KQ - 5 14", "c3d5 d8d5 f7d5 c6d5");
        case!(fork, false, "2r3k1/6p1/p2q1rRp/3pp3/3P1p1R/3Q3P/PP3PP1/6K1 w - - 0 31", "g6f6 d6f6 h4h5 e5e4 d3b3 g7g5 b3d5 f6f7 d5e4 c8c1 g1h2 f7h5");
        case!(fork, false, "r4rk1/pp2qppp/5p2/1b1p4/1b1Q4/2N1B3/PPP2PPP/2KR3R b - - 7 13", "b4c5 d4c5 e7c5 e3c5");
        case!(fork, false, "r2qr1k1/5p1p/pn3bp1/1p6/3P2bN/1P1B2PP/PB3PQ1/R3R1K1 b - - 0 19", "f6d4 e1e8 d8e8 b2d4");
        case!(fork, false, "6k1/7p/3R2p1/8/5p2/P4P2/1P1N2PP/3r1nK1 w - - 0 33", "d2e4 f1d2 g1f2 d2e4");
        case!(fork, true, "3r4/6pp/pNnNpk2/1p6/3pR3/8/PPP2PPK/8 w - - 4 32", "e4f4 f6e5 d6f7 e5f4 f7d8 c6d8");
    }

    #[test]
    fn trapped_cases() {
        case!(trapped_piece, true, "r4rk1/pp1nppbp/3p1n2/q4p2/8/N1P1PP2/PP1BB1PP/2RQ1RK1 b - - 0 13", "b7b6 e2b5 a7a6 c3c4 a5a3 b2a3");
        case!(trapped_piece, false, "r1b1k3/1pp4R/3p4/p2P4/2P5/8/PP2pKPP/8 b - - 1 34", "c8f5 h7h8 e8e7 h8a8 e2e1q f2e1");
        case!(trapped_piece, true, "rnb1k2r/ppppqppp/8/2b4n/4P1N1/2N5/PPPP1PPP/R1BQKB1R w KQkq - 3 6", "f2f3 e7h4 g2g3 h5g3 h2g3 h4h1");
        case!(trapped_piece, false, "2r2rk1/3bbpp1/p2p1n1p/1p1Pp3/4P3/5QNP/PPq2PPN/R1B1R1K1 w - - 6 19", "e1e2 c2d1 h2f1 c8c1 a1c1 d1c1");
        case!(trapped_piece, false, "3qr1k1/p5pp/1p3n2/3p2P1/2rQ4/5B1P/PBb2P2/2R2RK1 w - - 1 21", "f3d5 d8d5 d4d5 f6d5");
        case!(trapped_piece, true, "2kr4/Qp3ppp/1p1q1n2/4r3/8/8/PPP1B1PP/2K1R2R w - - 0 19", "e2d3 e5a5 a7a5 b6a5");
        case!(trapped_piece, true, "1r3rk1/4qppp/p1P1p3/Qp2P3/2n5/1R3BP1/P4P1P/1R4K1 w - - 3 30", "a5a6 b8b6 a6b6 c4b6");
        case!(trapped_piece, true, "r5k1/5Npp/8/3r4/4b3/2R2RP1/P5PP/6K1 w - - 1 28", "f3e3 a8a2 f7h6 g7h6");
        case!(trapped_piece, true, "6k1/pp2rpp1/2p4p/8/1Pr5/PB2PpP1/5PbP/1R2K1R1 b - - 3 28", "c4c3 e1d2 e7e3 f2e3 c3b3 b1b3");
        case!(trapped_piece, true, "r2q1r1k/pbp2pp1/3b1n1p/2p1Q3/8/2NB3P/PPP2PP1/R1B1R1K1 w - - 1 16", "e5f5 g7g6 f5f4 d6f4");
    }

    #[test]
    fn discovered_attack_cases() {
        case!(discovered_attack, false, "5rk1/2pqnrpp/p3p1b1/N3P3/1PRPPp2/P4Q2/3B1RPP/6K1 w - - 3 30", "d2f4 f7f4 f3f4 f8f4");
        case!(discovered_attack, false, "5rk1/3R4/p1p3pp/1p2b3/2P1n2q/4Q2P/PP3PP1/4R1K1 w - - 4 27", "e3e4 h4f2 g1h1 f2f1 e1f1 f8f1");
        case!(discovered_attack, true, "r2q1rk1/pppb1pbp/2n1pnp1/1BPpB3/3P4/4PN2/PP3PPP/RN1QK2R w KQ - 3 9", "e1g1 c6e5 d4e5 d7b5");
        case!(discovered_attack, true, "r4rk1/p1p1qppp/3b4/4n3/Q7/2NP4/PP3PPP/R1B2RK1 w - - 0 16", "f1e1 e5f3 g2f3 e7e1");
        case!(discovered_attack, true, "5r2/6k1/8/p1p1p1p1/Pp1p2P1/1P1PnN1P/2P1KR2/8 w - - 3 38", "f3e5 f8e8 e5c6 e3g4 e2f1 g4f2");
        case!(discovered_attack, false, "2r3k1/1r2pp1p/bqNp2p1/3P4/1p2P3/4bN2/1P4PP/2RQR2K w - - 0 24", "c6e7 b7e7 c1c8 a6c8");
        case!(discovered_attack, false, "4r2k/6pp/1R6/1pq5/8/P4QPP/1P3P1K/8 w - - 3 41", "f3c6 c5f2 c6g2 f2b6");
    }

    #[test]
    fn deflection_cases() {
        case!(deflection, true, "r1bqkbnr/pp3p1p/6p1/2pBp3/4P3/2P1B3/PP3PPP/RN1QK2R b KQkq - 0 9", "g8f6 d5f7 e8f7 d1d8");
        case!(deflection, false, "rnb1k2r/pppp2p1/4p2p/5p2/1q1Pn2P/2NQPN2/PPP2PP1/R3KB1R w KQkq - 1 9", "a2a3 b4b2 a1b1 b2c3 d3c3 e4c3");
        case!(deflection, false, "8/1R4p1/p5rp/4bN2/5kP1/2P4K/PP6/8 b - - 0 40", "g6g4 b7b4 f4f5 b4g4");
        case!(deflection, false, "5rk1/3R4/p1p3pp/1p2b3/2P1n2q/4Q2P/PP3PP1/4R1K1 w - - 4 27", "e3e4 h4f2 g1h1 f2f1 e1f1 f8f1");
        case!(deflection, false, "r2q2k1/pp4bp/3pnppn/3N4/4Pp1B/7P/PPPQ2P1/R4RK1 w - - 0 19", "d5f4 e6f4 f1f4 g6g5 d2d5 g8h8 f4f2 g5h4");
        case!(deflection, false, "r2k2r1/1b2nQb1/1p2p2p/p3Pp2/2P4q/P6P/NP2R1PN/2R4K b - - 0 26", "h4d4 a2c3 g8f8 f7g7 f8g8 g7h6");
        case!(deflection, false, "5r2/6k1/8/p1p1p1p1/Pp1p2P1/1P1PnN1P/2P1KR2/8 w - - 3 38", "f3e5 f8e8 e5c6 e3g4 e2f1 g4f2");
        case!(deflection, true, "r1bqkb1r/4pp1p/p1pp1np1/4P3/P1B5/2N5/1PP2PPP/R1BQK2R b KQkq - 0 9", "d6e5 c4f7 e8f7 d1d8");
        case!(deflection, true, "8/8/PR4K1/8/5k1P/r7/4p3/8 w - - 0 52", "b6e6 a3a6 e6a6 e2e1q");
        case!(deflection, true, "8/R4pk1/6p1/P6p/3n3P/5PK1/r4NP1/8 w - - 3 43", "a5a6 d4f5 g3h2 a2f2");
    }

    #[test]
    fn skewer_cases() {
        case!(skewer, true, "3r4/6p1/5r1p/7k/3N1P2/3K2P1/3R4/3R4 w - - 1 50", "d2e2 d8d4 d3d4 f6d6 d4e5 d6d1");
        case!(skewer, false, "5R2/p2rkpKp/1p2p1p1/4P1P1/8/8/P7/8 b - - 9 47", "a7a5 f8f7 e7e8 f7d7 e8d7 g7h7 b6b5 h7g6");
        case!(skewer, true, "1rr5/3k4/3bpp2/q5p1/PpRP3p/1P3N1P/1K1Q1PP1/7R w - - 2 25", "h1c1 d6f4 d2d3 f4c1");
        case!(skewer, true, "rn3rk1/p2p3p/1pb1p1pn/4Q3/P1B5/qNP2PP1/3N3P/4RRK1 b - - 2 21", "c6a4 e1a1 a3e7 a1a4");
        case!(skewer, true, "3k4/R7/8/1BK3p1/P1P2bPp/6r1/8/8 w - - 3 67", "a4a5 f4e3 c5c6 e3a7");
        case!(skewer, true, "7r/3q4/5k1p/8/4pp2/2Q5/P1P3PP/6K1 b - - 1 35", "f6f5 c3h3 f5f6 h3d7");
    }

    #[test]
    fn interference_xray_quiet_cases() {
        case!(interference, false, "6k1/1b1q1pbp/4pnp1/2Pp4/rp1P1P2/3BPRNP/4Q1P1/4B1K1 b - - 1 26", "f6e4 d3b5 b7c6 b5a4");
        case!(interference, true, "r5k1/ppp2r2/3p3p/3Pp3/1P2N1bb/R5N1/1P3P1K/6R1 b - - 5 25", "g4f3 g3f5 g8h7 a3f3");
        case!(x_ray, true, "5R2/8/p1p4p/1p1p2k1/6r1/1P2P1r1/P1PKR3/8 b - - 3 33", "g3g2 f8g8 g5f6 e2g2 g4g2 g8g2");
        case!(quiet_move, false, "7r/3k4/1P3p2/1K1Pp1p1/2N1P1P1/8/8/8 b - - 2 49", "h8h4 b6b7 h4h1 b7b8n");
    }

    #[test]
    fn pin_cases() {
        case!(pin_prevents_attack, true, "2k5/p7/bpq1p3/8/2PP2P1/1K2P1p1/4Q1P1/8 b - - 4 36", "a6c4 e2c4 c6c4 b3c4");
        case!(pin_prevents_attack, true, "r2q1r1k/pp3pp1/2p2n1p/3PB2b/3P4/1B5P/P1PQ1PP1/R3R1K1 b - - 0 18", "f6d5 d2h6 h8g8 h6g7");
        case!(pin_prevents_attack, false, "r4r2/pp3pkp/2p5/3pPp1q/3p1P2/3Q1R2/PPP3PP/R5K1 b - - 3 18", "c6c5 f3h3 h5g6 h3g3 g7h8 g3g6");
        case!(pin_prevents_attack, false, "r2q4/4b1kp/6p1/2ppPr2/3P4/2P2N2/P4RQP/R5K1 w - - 0 27", "f3d2 f5g5 d2f3 g5g2");
        case!(pin_prevents_attack, false, "1kr5/p3R3/7p/5Pp1/6P1/6K1/PP1R1P1P/6r1 w - - 1 32", "g3h3 h6h5 g4h5 c8h8 e7e8 h8e8");
        case!(pin_prevents_escape, false, "2k5/p7/bpq1p3/8/2PP2P1/1K2P1p1/4Q1P1/8 b - - 4 36", "a6c4 e2c4 c6c4 b3c4");
        case!(pin_prevents_escape, false, "r2q1r1k/pp3pp1/2p2n1p/3PB2b/3P4/1B5P/P1PQ1PP1/R3R1K1 b - - 0 18", "f6d5 d2h6 h8g8 h6g7");
        case!(pin_prevents_escape, true, "r4r2/pp3pkp/2p5/3pPp1q/3p1P2/3Q1R2/PPP3PP/R5K1 b - - 3 18", "c6c5 f3h3 h5g6 h3g3 g7h8 g3g6");
        case!(pin_prevents_escape, true, "r2q4/4b1kp/6p1/2ppPr2/3P4/2P2N2/P4RQP/R5K1 w - - 0 27", "f3d2 f5g5 d2f3 g5g2");
        case!(pin_prevents_escape, false, "1kr5/p3R3/7p/5Pp1/6P1/6K1/PP1R1P1P/6r1 w - - 1 32", "g3h3 h6h5 g4h5 c8h8 e7e8 h8e8");
        case!(pin_prevents_escape, false, "2r5/1KP5/8/4k3/7p/7P/4p3/2R5 b - - 1 49", "c8e8 c1e1 e5f4 e1e2 e8e2 c7c8q");
    }

    #[test]
    fn hanging_and_pawn_cases() {
        case!(hanging_piece, true, "r2qr1k1/1p3ppp/p1p2nb1/8/4P3/1P5P/PBQN1PP1/R3R1K1 w - - 1 17", "c2c4 d8d2 b2f6 g7f6");
        case!(hanging_piece, true, "8/p4p2/2p2Pk1/1p1p2pp/1P4P1/2P4P/2r2R2/5K2 b - - 1 40", "h5g4 f2c2");
        case!(hanging_piece, true, "2B2k2/pp5p/2p5/2n1p3/1PPbPp1q/P6P/4Q1P1/3N3K b - - 0 28", "c5e4 e2e4");
        case!(hanging_piece, true, "2r3k1/5ppp/bq2p3/p2pPnP1/5PR1/NP3NbP/P2Q4/2BK4 b - - 0 27", "b6g1 f3g1");
        case!(advanced_pawn, false, "4r3/R1p2k2/3p1pp1/2r2p1p/1pN2Pn1/1P2PKP1/2P3P1/4R3 b - - 3 39", "d6d5 c4d6 f7e7 d6e8");
        case!(advanced_pawn, false, "1R6/6kp/4Pp1q/3P4/R1P5/P5pP/6P1/7K w - - 1 34", "e6e7 h6c1");
        case!(advanced_pawn, true, "2R5/2P2kpp/8/1p4b1/4n3/P6P/2p2PPK/2B5 b - - 0 41", "g5c1 c8f8 f7f8 c7c8q");
        case!(advanced_pawn, false, "6r1/pppq3k/2np2np/8/3P2pB/N1PR1p2/PP2QPBN/6K1 w - - 0 33", "g2f3 g4f3 e2f1 g6h4");
    }

    #[test]
    fn endgame_cases() {
        let rook = |fen: &str, line: &str| piece_endgame(&make(fen, line), Role::Rook);
        assert!(!rook("8/p5KP/k7/6R1/6P1/1p6/8/7r w - - 0 44", "h7h8q h1h8 g7h8 b3b2 g5h5 b2b1q"));
        assert!(!rook("8/4R1P1/8/3r4/6K1/8/4p3/3k4 b - - 0 62", "e2e1q e7e1 d1e1 g7g8q"));
        assert!(rook("8/5p2/5k2/p4p2/8/1PPp1R2/r7/3K2R1 w - - 1 36", "f3d3 a2a1 d1d2 a1g1"));
        assert!(!rook("8/pppk4/3p4/3P1p1p/PP3Rr1/4PpPK/5P2/8 w - - 5 36", "f4g4 h5g4 h3h4 c7c5 d5c6 b7c6 h4g5 d7e6"));
    }

    #[test]
    fn intermezzo_cases() {
        case!(intermezzo, true, "8/5rpk/7p/8/3Q4/B4NKP/R2n2P1/5q2 b - - 3 42", "d2f3 d4e4 g7g6 g2f3");
        case!(intermezzo, true, "6k1/4rpp1/3r3p/p2N4/PbB5/1Pq2Q1P/R2p1PP1/3R2K1 b - - 8 31", "c3f3 d5e7 g8f8 g2f3");
        case!(intermezzo, false, "3r2k1/p3bqpp/2b1p3/2p2p2/8/2PNB1QP/PP3PP1/R5K1 w - - 2 26", "d3c5 f5f4 e3f4 e7c5");
        case!(intermezzo, false, "4b2r/r6k/6p1/3BQ1Rn/3P1P1P/p1qN4/2P5/2K5 b - - 0 33", "c3c7 g5h5 g6h5 d5e4 e8g6 e5h5 h7g8 h5g6");
    }

    #[test]
    fn back_rank_cases() {
        case!(back_rank_mate, true, "5r1k/4q1p1/p2pP2p/1p6/1P2Q3/PB6/1BP3PP/6K1 w - - 1 27", "e4g6 e7a7 b2d4 a7d4 g1h1 f8f1");
        case!(back_rank_mate, false, "3r2k1/1bQ3p1/p2p3p/3qp1b1/1p6/1P1B4/P1P3PP/1K3R2 b - - 4 25", "d5c6 c7f7 g8h8 f7f8 d8f8 f1f8");
        case!(back_rank_mate, true, "r5k1/pQ3ppp/8/8/B1pp4/4q3/PP5P/5R1K b - - 0 26", "a8d8 b7f7 g8h8 f7f8 d8f8 f1f8");
        case!(back_rank_mate, false, "3r2k1/1b4pp/1p2pr2/p5N1/8/PP2n1P1/1BR2bBP/4R2K w - - 1 27", "b2f6 b7g2");
    }

    #[test]
    fn side_attack_cases() {
        case!(kingside_attack, false, "rn1qk2r/5ppp/2p1p3/pp3bN1/1b1P2n1/1QN1PP2/P3B2P/R1B2RK1 w kq - 0 13", "f3g4 d8g5 e3e4 g5d8 e4f5 d8d4");
        case!(kingside_attack, false, "6k1/1p4p1/p1p4p/3p1rq1/3Pp1N1/2P5/PP2K1Q1/5R2 w - - 0 39", "g4h6 g7h6 g2g5 f5g5");
        case!(kingside_attack, true, "3r2k1/1p3ppp/pq6/8/P3B3/5PNb/1PP1Qb1P/R1B3K1 w - - 0 25", "e2f2 d8d1 g3f1 d1f1");
        case!(kingside_attack, true, "r4rk1/bpp3p1/p2p2qp/3bp3/1P6/P1PP3P/1B1Q1PPN/R4RK1 w - - 1 21", "g2g3 g6g3");
        case!(kingside_attack, true, "r1b1kb2/pp1n1p2/4p3/3pP2r/3n4/3B1N1q/PP3P1P/R1BQ1RK1 w q - 0 17", "f3d4 h3h2");
        case!(kingside_attack, true, "rn1q1rk1/pp1bbpp1/2p4p/2PpN3/3PnN1P/3B1P2/PPQ3P1/R1B2RK1 b - - 0 15", "e4g3 d3h7 g8h8 e5f7 f8f7 f4g6 h8h7 g6f8 h7g8 c2h7 g8f8 h7h8");
        case!(kingside_attack, false, "2r2rk1/1q3pp1/p3p2p/1p1N1b2/8/P3PQ2/1P3PPP/R2R2K1 b - - 0 25", "f5c2 d5f6 g7f6 f3b7");
        assert!(!cook(&make("r1b1k2r/pppp2pp/5n2/b3q3/8/2PBP3/PP4PP/RNBQ1RK1 w kq - 3 11", "b1d2 e5e3 g1h1 e3d3")).contains(&"kingsideAttack"));
        case!(queenside_attack, true, "2k2b2/1p3b1p/2p2p2/1p1qp3/6PN/1P2Q2P/P1P2P2/2KB4 w - - 1 28", "h4f5 f8a3 c1b1 d5d1 e3c1 d1c1");
        case!(queenside_attack, true, "k2r1b2/ppR1p1p1/7r/4B2p/8/1P3B2/P2PK1PP/8 b - - 2 25", "d8b8 f3b7 b8b7 c7c8 b7b8 c8b8");
        case!(queenside_attack, false, "1kq5/pp3pQ1/2p5/4p3/4Pn1p/5P1P/1PP2P1K/5B2 b - - 0 38", "c8c7 g7h8 c7c8 h8e5");
        case!(queenside_attack, false, "3rr1k1/p5pp/2p5/8/1P1bbp2/P1PP1B2/6PP/RK3R2 w - - 0 28", "c3d4 e4d3 b1b2 d3f1");
        case!(queenside_attack, false, "r4r1k/5p1p/5qp1/p3b1RP/1p3P2/8/PP1BQ3/2K4R w - - 0 32", "g5e5 f6c6 c1d1 c6h1");
    }

    #[test]
    fn promotion_cases() {
        case!(under_promotion, false, "8/1Pp3p1/8/2p5/2P5/5kbp/3p4/7K w - - 0 52", "b7b8q d2d1r");
        case!(under_promotion, true, "3R3r/p1P1kp1b/4pnpp/7P/6P1/2p5/P4P2/3R2K1 b - - 0 31", "c3c2 c7c8n");
        case!(under_promotion, true, "6k1/p7/4pr2/2P3r1/4Bp1q/1Q3PpP/P4bP1/3R1R1K w - - 1 33", "d1d7 h4h3 g2h3 g3g2 h1h2 g2f1n h2h1 g5g1");
        case!(under_promotion, true, "R7/P7/8/8/6k1/7p/r7/5K2 b - - 0 51", "g4g3 a8g8 g3h2 a7a8r");
        case!(under_promotion, true, "6k1/P5P1/1n4K1/8/8/8/8/8 b - - 2 68", "b6c8 a7a8b c8e7 g6f6");
        case!(under_promotion, false, "3B4/1pp5/p1b1B3/P3Q1N1/1P5k/2P5/R3R1p1/1q4K1 w - - 2 39", "g1h2 g2g1b");
        case!(under_promotion, false, "2kr4/2pR4/2P1K1P1/8/8/p3n3/8/8 b - - 1 49", "a3a2 d7d8 c8d8 g6g7 a2a1q g7g8q");
    }

    #[test]
    fn collinear_cases() {
        case!(collinear, true, "r1bq1r1k/1p2bp1n/p1n1p2Q/8/3P3B/2N5/PP2BPPP/2R2RK1 b - - 0 1", "a6a5 h4f6");
        case!(collinear, true, "3r2k1/1p3ppp/2p1p3/p3P2b/Pb2PP2/1QN3Pq/1P2B2P/5R1K w - - 0 23", "f1d1 d8d2 d1d2 h5f3 e2f3 h3f1");
        case!(collinear, true, "2b1k2r/pp2P3/2p3RQ/3r4/5P2/7P/Pq4PK/4R3 b - - 2 40", "d5d2 h6h5");
        case!(collinear, false, "4k3/8/8/8/1r5Q/8/8/R5K1 w - - 0 1", "a1e1 b4e4");
    }

    #[test]
    fn trapped_util_cases() {
        let trapped = |fen: &str| is_trapped(&chess::parse_fen(fen).unwrap(), Square::H5);
        assert!(!trapped("q3k3/7p/8/4N2q/3PP3/4B3/8/4K2R b - - 0 1"));
        assert!(trapped("q3k3/7p/8/4N2q/3PP3/4B3/7R/4K2R b - - 0 1"));
        assert!(!trapped("q3k3/7p/8/4N2b/3PP3/4B3/7R/4K2R b - - 0 1"));
        assert!(!trapped("4k3/7p/8/4N2q/3PP2p/4B3/8/4K3 b - - 0 1"));
        // Upstream omits the black king (python-chess allows it); a distant one changes nothing.
        assert!(trapped("k7/3P4/8/4N2b/7p/6N1/8/4K3 b - - 0 1"));
    }
}
