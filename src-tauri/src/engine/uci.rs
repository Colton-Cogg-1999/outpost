//! Pure parsing of UCI engine output. No I/O here, so every case is unit-testable.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum Score {
    /// Centipawns.
    Cp(i32),
    /// Mate in N moves (positive: side to move mates; negative: gets mated; 0: mated now).
    Mate(i32),
}

impl Score {
    pub fn negate(self) -> Score {
        match self {
            Score::Cp(cp) => Score::Cp(-cp),
            Score::Mate(n) => Score::Mate(-n),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    pub depth: Option<u32>,
    pub seldepth: Option<u32>,
    pub multipv: Option<u32>,
    pub score: Option<Score>,
    /// Set for `lowerbound` / `upperbound` scores from aspiration windows.
    pub bound: bool,
    pub wdl: Option<[u32; 3]>,
    pub nodes: Option<u64>,
    pub nps: Option<u64>,
    pub time_ms: Option<u64>,
    pub hashfull: Option<u32>,
    pub tbhits: Option<u64>,
    pub pv: Vec<String>,
    pub string: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UciOption {
    pub name: String,
    /// check | spin | combo | button | string
    pub kind: String,
    pub default: Option<String>,
    pub min: Option<i64>,
    pub max: Option<i64>,
    pub vars: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EngineLine {
    Id { key: String, value: String },
    Option(UciOption),
    UciOk,
    ReadyOk,
    Info(Info),
    BestMove { best: Option<String>, ponder: Option<String> },
    Other,
}

fn num<T: std::str::FromStr>(tokens: &[&str], i: usize) -> Option<T> {
    tokens.get(i).and_then(|t| t.parse().ok())
}

pub fn parse_line(line: &str) -> EngineLine {
    let line = line.trim();
    let tokens: Vec<&str> = line.split_whitespace().collect();
    match tokens.first().copied() {
        Some("id") if tokens.len() >= 3 => EngineLine::Id { key: tokens[1].to_string(), value: tokens[2..].join(" ") },
        Some("option") => parse_option(&tokens).map(EngineLine::Option).unwrap_or(EngineLine::Other),
        Some("uciok") => EngineLine::UciOk,
        Some("readyok") => EngineLine::ReadyOk,
        Some("info") => EngineLine::Info(parse_info(&tokens)),
        Some("bestmove") => {
            let best = tokens.get(1).filter(|m| **m != "(none)" && **m != "0000").map(|s| s.to_string());
            let ponder = match tokens.get(2) {
                Some(&"ponder") => tokens.get(3).map(|s| s.to_string()),
                _ => None,
            };
            EngineLine::BestMove { best, ponder }
        }
        _ => EngineLine::Other,
    }
}

fn parse_info(tokens: &[&str]) -> Info {
    let mut info = Info::default();
    let mut i = 1;
    while i < tokens.len() {
        match tokens[i] {
            "depth" => info.depth = num(tokens, i + 1),
            "seldepth" => info.seldepth = num(tokens, i + 1),
            "multipv" => info.multipv = num(tokens, i + 1),
            "nodes" => info.nodes = num(tokens, i + 1),
            "nps" => info.nps = num(tokens, i + 1),
            "time" => info.time_ms = num(tokens, i + 1),
            "hashfull" => info.hashfull = num(tokens, i + 1),
            "tbhits" => info.tbhits = num(tokens, i + 1),
            "score" => {
                let value = num::<i32>(tokens, i + 2);
                info.score = match (tokens.get(i + 1).copied(), value) {
                    (Some("cp"), Some(v)) => Some(Score::Cp(v)),
                    (Some("mate"), Some(v)) => Some(Score::Mate(v)),
                    _ => None,
                };
                i += 3;
                while let Some(&t) = tokens.get(i) {
                    if t == "lowerbound" || t == "upperbound" {
                        info.bound = true;
                        i += 1;
                    } else {
                        break;
                    }
                }
                continue;
            }
            "wdl" => {
                if let (Some(w), Some(d), Some(l)) = (num(tokens, i + 1), num(tokens, i + 2), num(tokens, i + 3)) {
                    info.wdl = Some([w, d, l]);
                }
                i += 4;
                continue;
            }
            "pv" => {
                info.pv = tokens[i + 1..].iter().map(|s| s.to_string()).collect();
                break;
            }
            "string" => {
                info.string = Some(tokens[i + 1..].join(" "));
                break;
            }
            // Single-value fields we do not use.
            "currmove" | "currmovenumber" | "cpuload" | "sbhits" | "refutation" | "currline" => {}
            _ => {
                i += 1;
                continue;
            }
        }
        i += 2;
    }
    info
}

/// `option name <name with spaces> type <t> [default <x>] [min <x>] [max <x>] [var <x>]*`
fn parse_option(tokens: &[&str]) -> Option<UciOption> {
    let keywords = ["name", "type", "default", "min", "max", "var"];
    let mut fields: Vec<(&str, String)> = Vec::new();
    let mut current: Option<(&str, Vec<&str>)> = None;
    for &token in &tokens[1..] {
        // Inside a name, words like "type" still end it; everywhere else keywords start fields.
        if keywords.contains(&token) {
            if let Some((key, words)) = current.take() {
                fields.push((key, words.join(" ")));
            }
            current = Some((token, Vec::new()));
        } else if let Some((_, words)) = current.as_mut() {
            words.push(token);
        }
    }
    if let Some((key, words)) = current {
        fields.push((key, words.join(" ")));
    }
    let get = |k: &str| fields.iter().find(|(key, _)| *key == k).map(|(_, v)| v.clone());
    let name = get("name").filter(|n| !n.is_empty())?;
    let kind = get("type")?;
    Some(UciOption {
        name,
        kind,
        default: get("default").map(|d| if d == "<empty>" { String::new() } else { d }),
        min: get("min").and_then(|v| v.parse().ok()),
        max: get("max").and_then(|v| v.parse().ok()),
        vars: fields.iter().filter(|(k, _)| *k == "var").map(|(_, v)| v.clone()).collect(),
    })
}

/// Option names and values are written into the engine's stdin. Strip anything that
/// could end the line early and inject a second command.
pub fn sanitize(value: &str) -> String {
    value.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string()
}

pub fn setoption(name: &str, value: Option<&str>) -> String {
    match value {
        Some(v) => format!("setoption name {} value {}", sanitize(name), sanitize(v)),
        None => format!("setoption name {}", sanitize(name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stockfish_info_with_wdl() {
        let line = "info depth 12 seldepth 19 multipv 1 score cp 31 wdl 61 934 5 nodes 57185 nps 1429625 hashfull 17 tbhits 0 time 40 pv g1f3 b8c6 d2d4";
        let EngineLine::Info(info) = parse_line(line) else { panic!() };
        assert_eq!(info.depth, Some(12));
        assert_eq!(info.seldepth, Some(19));
        assert_eq!(info.multipv, Some(1));
        assert_eq!(info.score, Some(Score::Cp(31)));
        assert_eq!(info.wdl, Some([61, 934, 5]));
        assert_eq!(info.nodes, Some(57185));
        assert_eq!(info.time_ms, Some(40));
        assert_eq!(info.pv, ["g1f3", "b8c6", "d2d4"]);
        assert!(!info.bound);
    }

    #[test]
    fn parses_mate_scores_and_bounds() {
        let EngineLine::Info(info) = parse_line("info depth 20 score mate -3 nodes 10 pv e1e2") else { panic!() };
        assert_eq!(info.score, Some(Score::Mate(-3)));
        let EngineLine::Info(info) = parse_line("info depth 20 score cp 15 lowerbound nodes 10 pv e2e4") else { panic!() };
        assert!(info.bound);
        assert_eq!(info.nodes, Some(10));
        let EngineLine::Info(info) = parse_line("info depth 0 score mate 0") else { panic!() };
        assert_eq!(info.score, Some(Score::Mate(0)));
        assert!(info.pv.is_empty());
    }

    #[test]
    fn parses_info_string_and_currmove() {
        let EngineLine::Info(info) = parse_line("info string NNUE evaluation using nn.nnue") else { panic!() };
        assert_eq!(info.string.as_deref(), Some("NNUE evaluation using nn.nnue"));
        let EngineLine::Info(info) = parse_line("info depth 5 currmove e2e4 currmovenumber 1") else { panic!() };
        assert_eq!(info.depth, Some(5));
        assert!(info.pv.is_empty());
    }

    #[test]
    fn parses_bestmove_variants() {
        assert_eq!(parse_line("bestmove e2e4 ponder e7e5"), EngineLine::BestMove { best: Some("e2e4".into()), ponder: Some("e7e5".into()) });
        assert_eq!(parse_line("bestmove (none)"), EngineLine::BestMove { best: None, ponder: None });
        assert_eq!(parse_line("bestmove 0000"), EngineLine::BestMove { best: None, ponder: None });
        assert_eq!(parse_line("bestmove a7a8q"), EngineLine::BestMove { best: Some("a7a8q".into()), ponder: None });
    }

    #[test]
    fn parses_options() {
        let EngineLine::Option(o) = parse_line("option name Hash type spin default 16 min 1 max 33554432") else { panic!() };
        assert_eq!((o.name.as_str(), o.kind.as_str(), o.default.as_deref(), o.min, o.max), ("Hash", "spin", Some("16"), Some(1), Some(33554432)));
        let EngineLine::Option(o) = parse_line("option name Skill Level type spin default 20 min 0 max 20") else { panic!() };
        assert_eq!(o.name, "Skill Level");
        let EngineLine::Option(o) = parse_line("option name Clear Hash type button") else { panic!() };
        assert_eq!(o.kind, "button");
        let EngineLine::Option(o) = parse_line("option name Backend type combo default cuda var cuda var opencl var eigen") else { panic!() };
        assert_eq!(o.vars, ["cuda", "opencl", "eigen"]);
        let EngineLine::Option(o) = parse_line("option name SyzygyPath type string default <empty>") else { panic!() };
        assert_eq!(o.default.as_deref(), Some(""));
        let EngineLine::Option(o) = parse_line("option name WeightsFile type string default <autodiscover>") else { panic!() };
        assert_eq!(o.default.as_deref(), Some("<autodiscover>"));
    }

    #[test]
    fn id_lines() {
        assert_eq!(parse_line("id name Stockfish 19"), EngineLine::Id { key: "name".into(), value: "Stockfish 19".into() });
    }

    #[test]
    fn setoption_cannot_inject_commands() {
        let cmd = setoption("Hash\nquit", Some("64\ngo infinite"));
        assert!(!cmd.contains('\n'));
        assert_eq!(cmd, "setoption name Hashquit value 64go infinite");
    }
}
