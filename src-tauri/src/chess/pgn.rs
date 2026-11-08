//! PGN import. Every game is replayed move by move; the stored PGN is regenerated
//! from validated moves (variations, comments and NAGs preserved), so the library
//! can never hold notation that disagrees with the board.

use super::{parse_fen, san_string, to_fen, uci_string};
use pgn_reader::{Nag, Outcome, RawComment, RawTag, Reader, SanPlus, Skip, Visitor};
use serde::Serialize;
use sha2::{Digest, Sha256};
use shakmaty::{Chess, Color, Position};
use std::ops::ControlFlow;

/// Seven Tag Roster order, then everything else in original order.
const ROSTER: [&str; 7] = ["Event", "Site", "Date", "Round", "White", "Black", "Result"];

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedGame {
    pub headers: Vec<(String, String)>,
    /// Present only when the game starts from a non-standard position.
    pub start_fen: Option<String>,
    pub moves_san: Vec<String>,
    pub moves_uci: Vec<String>,
    pub result: String,
    pub pgn: String,
    /// First problem found. The mainline is truncated at that point.
    pub error: Option<String>,
}

impl ParsedGame {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.is_empty() && *v != "?" && *v != "????.??.??")
    }

    /// Identifies the same game imported from different places (a pasted PGN and an
    /// API import of the same game): players, date and the exact move sequence.
    pub fn fingerprint(&self) -> String {
        let mut hash = Sha256::new();
        for part in [
            self.start_fen.as_deref().unwrap_or(""),
            &self.header("White").unwrap_or("").to_lowercase(),
            &self.header("Black").unwrap_or("").to_lowercase(),
            self.header("Date").or(self.header("UTCDate")).unwrap_or(""),
            self.header("UTCTime").or(self.header("StartTime")).unwrap_or(""),
            &self.moves_uci.join(" "),
        ] {
            hash.update(part.as_bytes());
            hash.update([0]);
        }
        hex::encode(hash.finalize())
    }
}

enum Token {
    Move { number: u32, white: bool, san: String },
    Nag(u8),
    Comment(String),
    Open,
    Close,
}

#[derive(Clone)]
struct Frame {
    before: Option<Chess>,
    pos: Chess,
    /// Set after an illegal move inside a variation: the rest of it is dropped.
    dead: bool,
}

struct Movetext {
    headers: Vec<(String, String)>,
    start_fen: Option<String>,
    tokens: Vec<Token>,
    stack: Vec<Frame>,
    frame: Frame,
    mainline_broken: bool,
    san: Vec<String>,
    uci: Vec<String>,
    result: String,
    error: Option<String>,
}

struct Tags {
    headers: Vec<(String, String)>,
    start: Option<Result<Chess, String>>,
}

struct Builder;

fn clean_comment(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.replace('}', ")")
}

impl Visitor for Builder {
    type Tags = Tags;
    type Movetext = Movetext;
    type Output = ParsedGame;

    fn begin_tags(&mut self) -> ControlFlow<Self::Output, Self::Tags> {
        ControlFlow::Continue(Tags { headers: Vec::new(), start: None })
    }

    fn tag(&mut self, tags: &mut Tags, name: &[u8], value: RawTag<'_>) -> ControlFlow<Self::Output> {
        let name = String::from_utf8_lossy(name).to_string();
        let value = value.decode_utf8_lossy().to_string();
        if name.eq_ignore_ascii_case("FEN") {
            tags.start = Some(parse_fen(&value).map_err(|e| e.to_string()));
        }
        if tags.headers.len() < 64 && value.len() < 2000 {
            tags.headers.push((name, value));
        }
        ControlFlow::Continue(())
    }

    fn begin_movetext(&mut self, tags: Tags) -> ControlFlow<Self::Output, Self::Movetext> {
        let (pos, start_fen, error) = match tags.start {
            Some(Ok(pos)) => {
                let fen = to_fen(&pos);
                let custom = (fen != super::START_FEN).then_some(fen);
                (pos, custom, None)
            }
            Some(Err(e)) => (Chess::default(), None, Some(format!("FEN header: {e}"))),
            None => (Chess::default(), None, None),
        };
        let broken = error.is_some();
        ControlFlow::Continue(Movetext {
            headers: tags.headers,
            start_fen,
            tokens: Vec::new(),
            stack: Vec::new(),
            frame: Frame { before: None, pos, dead: false },
            mainline_broken: broken,
            san: Vec::new(),
            uci: Vec::new(),
            result: "*".into(),
            error,
        })
    }

    fn san(&mut self, mt: &mut Movetext, san_plus: SanPlus) -> ControlFlow<Self::Output> {
        let in_variation = !mt.stack.is_empty();
        if (!in_variation && mt.mainline_broken) || mt.frame.dead {
            return ControlFlow::Continue(());
        }
        match san_plus.san.to_move(&mt.frame.pos) {
            Ok(m) => {
                let pos = &mt.frame.pos;
                let san = san_string(pos, m);
                mt.tokens.push(Token::Move {
                    number: pos.fullmoves().get(),
                    white: pos.turn() == Color::White,
                    san: san.clone(),
                });
                if !in_variation {
                    mt.san.push(san);
                    mt.uci.push(uci_string(m));
                }
                let mut next = mt.frame.pos.clone();
                next.play_unchecked(m);
                mt.frame.before = Some(std::mem::replace(&mut mt.frame.pos, next));
            }
            Err(e) => {
                if !in_variation {
                    mt.mainline_broken = true;
                    let ply = mt.san.len() + 1;
                    mt.error.get_or_insert_with(|| format!("Move {ply} ({san_plus}) is not legal: {e}."));
                } else {
                    mt.frame.dead = true;
                    mt.tokens.push(Token::Comment("[invalid moves removed]".into()));
                }
            }
        }
        ControlFlow::Continue(())
    }

    fn nag(&mut self, mt: &mut Movetext, nag: Nag) -> ControlFlow<Self::Output> {
        if matches!(mt.tokens.last(), Some(Token::Move { .. } | Token::Nag(_))) {
            mt.tokens.push(Token::Nag(nag.0));
        }
        ControlFlow::Continue(())
    }

    fn comment(&mut self, mt: &mut Movetext, comment: RawComment<'_>) -> ControlFlow<Self::Output> {
        if (!mt.stack.is_empty() || !mt.mainline_broken) && !mt.frame.dead {
            let text = clean_comment(comment.as_bytes());
            if !text.is_empty() {
                mt.tokens.push(Token::Comment(text));
            }
        }
        ControlFlow::Continue(())
    }

    fn begin_variation(&mut self, mt: &mut Movetext) -> ControlFlow<Self::Output, Skip> {
        let in_variation = !mt.stack.is_empty();
        let Some(before) = mt.frame.before.clone() else { return ControlFlow::Continue(Skip(true)) };
        if (!in_variation && mt.mainline_broken) || mt.frame.dead || mt.stack.len() > 32 {
            return ControlFlow::Continue(Skip(true));
        }
        mt.stack.push(mt.frame.clone());
        mt.frame = Frame { before: None, pos: before, dead: false };
        mt.tokens.push(Token::Open);
        ControlFlow::Continue(Skip(false))
    }

    fn end_variation(&mut self, mt: &mut Movetext) -> ControlFlow<Self::Output> {
        if let Some(frame) = mt.stack.pop() {
            mt.frame = frame;
            mt.tokens.push(Token::Close);
        }
        ControlFlow::Continue(())
    }

    fn outcome(&mut self, mt: &mut Movetext, outcome: Outcome) -> ControlFlow<Self::Output> {
        mt.result = match outcome {
            Outcome::Known(known) => known.to_string(),
            Outcome::Unknown => "*".into(),
        };
        ControlFlow::Continue(())
    }

    fn end_game(&mut self, mt: Movetext) -> ParsedGame {
        let header_result = mt
            .headers
            .iter()
            .find(|(k, _)| k == "Result")
            .map(|(_, v)| v.clone())
            .filter(|v| matches!(v.as_str(), "1-0" | "0-1" | "1/2-1/2" | "*"));
        let result = if mt.mainline_broken { "*".to_string() } else if mt.result != "*" { mt.result.clone() } else { header_result.unwrap_or_else(|| "*".into()) };
        let pgn = write_pgn(&mt.headers, mt.start_fen.as_deref(), &mt.tokens, &result);
        ParsedGame {
            headers: mt.headers,
            start_fen: mt.start_fen,
            moves_san: mt.san,
            moves_uci: mt.uci,
            result,
            pgn,
            error: mt.error,
        }
    }
}

fn escape_tag(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"").replace(['\n', '\r'], " ")
}

fn write_pgn(headers: &[(String, String)], start_fen: Option<&str>, tokens: &[Token], result: &str) -> String {
    let mut out = String::new();
    let get = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
    for name in ROSTER {
        let value = if name == "Result" { Some(result) } else { get(name) };
        let default = if name == "Date" { "????.??.??" } else { "?" };
        out.push_str(&format!("[{name} \"{}\"]\n", escape_tag(value.unwrap_or(default))));
    }
    for (name, value) in headers {
        if ROSTER.contains(&name.as_str()) || name.eq_ignore_ascii_case("FEN") || name.eq_ignore_ascii_case("SetUp") {
            continue;
        }
        if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !name.is_empty() {
            out.push_str(&format!("[{name} \"{}\"]\n", escape_tag(value)));
        }
    }
    if let Some(fen) = start_fen {
        out.push_str("[SetUp \"1\"]\n");
        out.push_str(&format!("[FEN \"{fen}\"]\n"));
    }
    out.push('\n');

    let mut words: Vec<String> = Vec::new();
    let mut need_number = true;
    for token in tokens {
        match token {
            Token::Move { number, white, san } => {
                if *white {
                    words.push(format!("{number}. {san}"));
                } else if need_number {
                    words.push(format!("{number}... {san}"));
                } else {
                    words.push(san.clone());
                }
                need_number = false;
            }
            Token::Nag(n) => words.push(format!("${n}")),
            Token::Comment(text) => {
                words.push(format!("{{ {text} }}"));
                need_number = true;
            }
            Token::Open => {
                words.push("(".into());
                need_number = true;
            }
            Token::Close => {
                words.push(")".into());
                need_number = true;
            }
        }
    }
    words.push(result.to_string());
    // Join with spaces, but no space after "(" or before ")", wrap at ~80 columns.
    let mut line = String::new();
    let mut prev_open = false;
    for word in words {
        let sep = !(line.is_empty() || prev_open || word == ")");
        if sep && line.len() + 1 + word.len() > 80 {
            out.push_str(&line);
            out.push('\n');
            line.clear();
        } else if sep {
            line.push(' ');
        }
        prev_open = word == "(";
        line.push_str(&word);
    }
    out.push_str(&line);
    out.push('\n');
    out
}

/// Parses every game in `text` (up to `limit`). Games with errors are returned with
/// `error` set so the caller can report them; they keep the valid prefix.
pub fn parse_many(text: &str, limit: usize) -> Vec<ParsedGame> {
    // Some sites emit a UTF-8 BOM; strip it so the first tag is recognised.
    let text = text.trim_start_matches('\u{feff}');
    let mut reader = Reader::new(std::io::Cursor::new(text.as_bytes()));
    let mut games = Vec::new();
    let mut builder = Builder;
    while games.len() < limit {
        match reader.read_game(&mut builder) {
            Ok(Some(game)) => {
                // Skip empty fragments: no headers and no moves.
                if !game.headers.is_empty() || !game.moves_san.is_empty() {
                    games.push(game);
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    games
}

pub fn parse_one(text: &str) -> Option<ParsedGame> {
    parse_many(text, 1).into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUY: &str = r#"[Event "Casual"]
[Site "?"]
[Date "2024.01.23"]
[White "Alice"]
[Black "Bob"]
[Result "1-0"]
[WhiteElo "1500"]

1. e4 e5 2. Nf3 Nc6 3. Bb5 { The Spanish. } a6 (3... Nf6 4. O-O) 4. Ba4 Nf6 5. O-O Be7 $6 1-0
"#;

    #[test]
    fn parses_mainline_variations_and_comments() {
        let game = parse_one(RUY).unwrap();
        assert!(game.error.is_none(), "{:?}", game.error);
        assert_eq!(game.moves_san, ["e4", "e5", "Nf3", "Nc6", "Bb5", "a6", "Ba4", "Nf6", "O-O", "Be7"]);
        assert_eq!(game.moves_uci[8], "e1g1");
        assert_eq!(game.result, "1-0");
        assert_eq!(game.header("WhiteElo"), Some("1500"));
        assert!(game.pgn.contains("3. Bb5 { The Spanish. } 3... a6 (3... Nf6 4. O-O) 4. Ba4"), "{}", game.pgn);
        assert!(game.pgn.contains("Be7 $6 1-0"));
        // Round trip is stable.
        let again = parse_one(&game.pgn).unwrap();
        assert_eq!(again.pgn, game.pgn);
        assert_eq!(again.fingerprint(), game.fingerprint());
    }

    #[test]
    fn multiple_games_and_bom() {
        let text = format!("\u{feff}{RUY}\n\n[Event \"Second\"]\n[Result \"0-1\"]\n\n1. f3 e5 2. g4 Qh4# 0-1\n");
        let games = parse_many(&text, 100);
        assert_eq!(games.len(), 2);
        assert_eq!(games[1].moves_san.last().unwrap(), "Qh4#");
        assert_eq!(games[1].result, "0-1");
        assert_ne!(games[0].fingerprint(), games[1].fingerprint());
        assert_eq!(parse_many(&text, 1).len(), 1);
    }

    #[test]
    fn illegal_move_truncates_mainline_and_reports() {
        let game = parse_one("1. e4 e5 2. Ke3 Nc6 *").unwrap();
        assert_eq!(game.moves_san, ["e4", "e5"]);
        assert!(game.error.unwrap().contains("Move 3"));
        assert!(!game.pgn.contains("Ke3"));
    }

    #[test]
    fn invalid_variation_is_dropped_but_mainline_kept() {
        let game = parse_one("1. e4 (1. e5 d5) 1... e5 2. Nf3 *").unwrap();
        assert!(game.error.is_none());
        assert_eq!(game.moves_san, ["e4", "e5", "Nf3"]);
    }

    #[test]
    fn custom_start_position_and_promotion() {
        let pgn = "[FEN \"8/P6k/8/8/8/8/8/K7 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. a8=N Kg6 *";
        let game = parse_one(pgn).unwrap();
        assert_eq!(game.start_fen.as_deref(), Some("8/P6k/8/8/8/8/8/K7 w - - 0 1"));
        assert_eq!(game.moves_uci, ["a7a8n", "h7g6"]);
        assert!(game.pgn.contains("[FEN \"8/P6k/8/8/8/8/8/K7 w - - 0 1\"]"));
    }

    #[test]
    fn black_to_move_start_uses_ellipsis() {
        let pgn = "[FEN \"4k3/8/8/8/8/8/4P3/4K3 b - - 0 10\"]\n\n10... Kd7 11. e4 *";
        let game = parse_one(pgn).unwrap();
        assert!(game.pgn.contains("10... Kd7 11. e4 *"), "{}", game.pgn);
    }

    #[test]
    fn rejects_bad_fen_header() {
        let game = parse_one("[FEN \"not a fen\"]\n\n1. e4 *").unwrap();
        assert!(game.error.is_some());
        assert!(game.moves_san.is_empty());
    }

    #[test]
    fn escapes_header_quotes() {
        let game = parse_one("[Event \"A \\\"quoted\\\" event\"]\n\n1. d4 *").unwrap();
        assert!(game.pgn.contains(r#"[Event "A \"quoted\" event"]"#), "{}", game.pgn);
    }
}
