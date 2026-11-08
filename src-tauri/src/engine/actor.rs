//! One task per running engine owns its process. Requests arrive over a channel;
//! the actor guarantees that output from a superseded search never reaches anyone:
//! a new request sends `stop`, waits for that search's `bestmove`, and only then
//! issues `position` + `go` for the new one. Every event also carries a search id so
//! the UI can drop anything it no longer wants.

use super::process::{EngineIdentity, Process};
use super::uci::{parse_line, setoption, EngineLine, Info, Score};
use crate::chess;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use shakmaty::{Chess, Color, Position};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchLimit {
    pub depth: Option<u32>,
    pub movetime_ms: Option<u64>,
    pub nodes: Option<u64>,
}

impl SearchLimit {
    pub fn is_infinite(&self) -> bool {
        self.depth.is_none() && self.movetime_ms.is_none() && self.nodes.is_none()
    }

    fn go_command(&self) -> String {
        if self.is_infinite() {
            return "go infinite".into();
        }
        let mut parts = vec!["go".to_string()];
        if let Some(d) = self.depth {
            parts.push(format!("depth {}", d.clamp(1, 245)));
        }
        if let Some(t) = self.movetime_ms {
            parts.push(format!("movetime {}", t.clamp(10, 86_400_000)));
        }
        if let Some(n) = self.nodes {
            parts.push(format!("nodes {}", n.max(1)));
        }
        parts.join(" ")
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub search_id: u64,
    /// Starting FEN of the move sequence; None means the standard start position.
    pub fen: Option<String>,
    #[serde(default)]
    pub moves: Vec<String>,
    #[serde(default = "one")]
    pub multipv: u32,
    #[serde(default)]
    pub limit: SearchLimit,
}

fn one() -> u32 {
    1
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PvLine {
    pub multipv: u32,
    pub depth: u32,
    /// From White's point of view.
    pub score: Score,
    /// As reported by the engine (side to move's point of view).
    pub raw_score: Score,
    /// Win/draw/loss per mille from White's point of view, when the engine reports it.
    pub wdl: Option<[u32; 3]>,
    pub san: Vec<String>,
    pub uci: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub search_id: u64,
    pub engine: String,
    pub fen: String,
    pub depth: u32,
    pub seldepth: Option<u32>,
    pub nodes: Option<u64>,
    pub nps: Option<u64>,
    pub time_ms: Option<u64>,
    pub hashfull: Option<u32>,
    pub lines: Vec<PvLine>,
    /// Set when the position is already decided and no engine was consulted.
    pub terminal: Option<String>,
    /// Served from the local evaluation cache rather than a live search.
    #[serde(default)]
    pub cached: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SearchEvent {
    #[serde(rename_all = "camelCase")]
    Update { snapshot: Snapshot },
    #[serde(rename_all = "camelCase")]
    Done { snapshot: Snapshot, best_move: Option<String> },
    #[serde(rename_all = "camelCase")]
    Error { search_id: u64, message: String },
}

pub type Sink = mpsc::UnboundedSender<SearchEvent>;

enum Cmd {
    Search(SearchRequest, Sink),
    Stop,
    Configure(Vec<(String, Option<String>)>, oneshot::Sender<Result<()>>),
    Quit(oneshot::Sender<()>),
}

#[derive(Clone)]
pub struct EngineHandle {
    pub id: String,
    pub identity: Arc<EngineIdentity>,
    tx: mpsc::UnboundedSender<Cmd>,
}

impl EngineHandle {
    pub fn is_alive(&self) -> bool {
        !self.tx.is_closed()
    }

    pub fn search(&self, request: SearchRequest, sink: Sink) -> Result<()> {
        self.tx.send(Cmd::Search(request, sink)).map_err(|_| anyhow!("the engine is not running"))
    }

    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
    }

    pub async fn configure(&self, options: Vec<(String, Option<String>)>) -> Result<()> {
        let (ack, wait) = oneshot::channel();
        self.tx.send(Cmd::Configure(options, ack)).map_err(|_| anyhow!("the engine is not running"))?;
        wait.await.map_err(|_| anyhow!("the engine stopped"))?
    }

    pub async fn quit(&self) {
        let (ack, wait) = oneshot::channel();
        if self.tx.send(Cmd::Quit(ack)).is_ok() {
            let _ = tokio::time::timeout(Duration::from_secs(3), wait).await;
        }
    }

    /// Runs one finite search to completion and returns its final snapshot.
    pub async fn search_once(&self, request: SearchRequest) -> Result<Snapshot> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        self.search(request, tx)?;
        while let Some(event) = rx.recv().await {
            match event {
                SearchEvent::Done { snapshot, .. } => return Ok(snapshot),
                SearchEvent::Error { message, .. } => return Err(anyhow!(message)),
                SearchEvent::Update { .. } => {}
            }
        }
        Err(anyhow!("the engine stopped before finishing the search"))
    }
}

struct Active {
    request: SearchRequest,
    pos: Chess,
    fen: String,
    sink: Sink,
    lines: BTreeMap<u32, PvLine>,
    last: Info,
    depth: u32,
    dirty: bool,
    stopping: Option<Instant>,
    /// A newer request replaced this one; its output is discarded.
    superseded: bool,
}

impl Active {
    fn snapshot(&self, engine: &str) -> Snapshot {
        let mut lines: Vec<PvLine> = self.lines.values().cloned().collect();
        lines.sort_by_key(|l| l.multipv);
        lines.truncate(self.request.multipv as usize);
        Snapshot {
            search_id: self.request.search_id,
            engine: engine.to_string(),
            fen: self.fen.clone(),
            depth: self.depth,
            seldepth: self.last.seldepth,
            nodes: self.last.nodes,
            nps: self.last.nps,
            time_ms: self.last.time_ms,
            hashfull: self.last.hashfull,
            lines,
            terminal: None,
            cached: false,
        }
    }

    fn absorb(&mut self, info: Info) {
        if let (Some(score), false, false) = (info.score, info.bound, info.pv.is_empty()) {
            let (san, uci) = chess::pv_to_san(&self.pos, &info.pv);
            if !uci.is_empty() {
                let white = self.pos.turn() == Color::White;
                let multipv = info.multipv.unwrap_or(1);
                let depth = info.depth.unwrap_or(0);
                self.lines.insert(
                    multipv,
                    PvLine {
                        multipv,
                        depth,
                        score: if white { score } else { score.negate() },
                        raw_score: score,
                        wdl: info.wdl.map(|[w, d, l]| if white { [w, d, l] } else { [l, d, w] }),
                        san,
                        uci,
                    },
                );
                if multipv == 1 {
                    self.depth = depth;
                }
                self.dirty = true;
            }
        }
        if info.nodes.is_some() {
            self.last = info;
        }
    }
}

/// Validates a request. Returns the final position and, if the game is already over
/// there, a finished snapshot so the engine is never asked to search a dead position.
fn prepare(request: &SearchRequest, engine: &str) -> Result<(Chess, String, Option<Snapshot>)> {
    let positions = chess::replay(request.fen.as_deref(), &request.moves)?;
    let pos = positions.last().cloned().unwrap_or_default();
    let fen = chess::to_fen(&pos);
    let status = chess::status(&pos);
    if status.over {
        let reason = status.reason.unwrap_or_default();
        let lines = if reason == "checkmate" {
            // UCI says "mate 0" (side to move is mated), which has no sign from White's
            // point of view. Encode the finished game as mate-in-1 for the winner; the UI
            // shows the result from `terminal` instead of a mate count.
            let score = if pos.turn() == Color::White { Score::Mate(-1) } else { Score::Mate(1) };
            vec![PvLine { multipv: 1, depth: 0, score, raw_score: Score::Mate(0), wdl: None, san: vec![], uci: vec![] }]
        } else {
            vec![PvLine { multipv: 1, depth: 0, score: Score::Cp(0), raw_score: Score::Cp(0), wdl: None, san: vec![], uci: vec![] }]
        };
        let snapshot = Snapshot { search_id: request.search_id, engine: engine.into(), fen: fen.clone(), lines, terminal: Some(reason), ..Default::default() };
        return Ok((pos, fen, Some(snapshot)));
    }
    Ok((pos, fen, None))
}

fn position_command(request: &SearchRequest) -> String {
    let base = match request.fen.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
        Some(fen) => format!("position fen {}", chess::parse_fen(fen).map(|p| chess::to_fen(&p)).unwrap_or_default()),
        None => "position startpos".to_string(),
    };
    if request.moves.is_empty() {
        base
    } else {
        format!("{base} moves {}", request.moves.join(" "))
    }
}

pub async fn start(id: String, mut process: Process, identity: EngineIdentity, options: Vec<(String, Option<String>)>) -> Result<EngineHandle> {
    for (name, value) in &options {
        process.send(&setoption(name, value.as_deref())).await?;
    }
    process.ready(Duration::from_secs(30)).await?;
    let has_multipv = identity.options.iter().any(|o| o.name.eq_ignore_ascii_case("MultiPV"));
    let (tx, rx) = mpsc::unbounded_channel();
    let handle = EngineHandle { id, identity: Arc::new(identity), tx };
    let name = handle.identity.name.clone();
    tokio::spawn(run(process, rx, name, has_multipv));
    Ok(handle)
}

async fn run(mut process: Process, mut rx: mpsc::UnboundedReceiver<Cmd>, engine: String, has_multipv: bool) {
    let mut active: Option<Active> = None;
    let mut pending: Option<(SearchRequest, Chess, String, Sink)> = None;
    let mut multipv_set = 1u32;
    let mut tick = tokio::time::interval(Duration::from_millis(120));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    macro_rules! begin {
        ($req:expr, $pos:expr, $fen:expr, $sink:expr) => {{
            let req: SearchRequest = $req;
            let legal = $pos.legal_moves().len() as u32;
            let multipv = req.multipv.clamp(1, 64).min(legal.max(1));
            let mut ok = true;
            if has_multipv && multipv != multipv_set {
                ok &= process.send(&setoption("MultiPV", Some(&multipv.to_string()))).await.is_ok();
                multipv_set = multipv;
            }
            let mut req = req;
            if !has_multipv {
                req.multipv = 1;
            } else {
                req.multipv = multipv;
            }
            ok &= process.send(&position_command(&req)).await.is_ok();
            ok &= process.send(&req.limit.go_command()).await.is_ok();
            if !ok {
                let _ = $sink.send(SearchEvent::Error { search_id: req.search_id, message: "Lost contact with the engine.".into() });
            }
            active = Some(Active {
                request: req,
                pos: $pos,
                fen: $fen,
                sink: $sink,
                lines: BTreeMap::new(),
                last: Info::default(),
                depth: 0,
                dirty: false,
                stopping: None,
                superseded: false,
            });
        }};
    }

    loop {
        tokio::select! {
            cmd = rx.recv() => match cmd {
                None => { process.quit().await; return; }
                Some(Cmd::Quit(ack)) => {
                    process.quit().await;
                    let _ = ack.send(());
                    return;
                }
                Some(Cmd::Search(request, sink)) => {
                    match prepare(&request, &engine) {
                        Err(e) => { let _ = sink.send(SearchEvent::Error { search_id: request.search_id, message: format!("{e:#}") }); }
                        Ok((_, _, Some(snapshot))) => {
                            // Decided position: answer at once, and silence any running search.
                            if let Some(a) = active.as_mut() {
                                a.superseded = true;
                                if a.stopping.is_none() { let _ = process.send("stop").await; a.stopping = Some(Instant::now()); }
                            }
                            pending = None;
                            let _ = sink.send(SearchEvent::Done { snapshot, best_move: None });
                        }
                        Ok((pos, fen, None)) => {
                            if let Some(a) = active.as_mut() {
                                a.superseded = true;
                                if a.stopping.is_none() { let _ = process.send("stop").await; a.stopping = Some(Instant::now()); }
                                pending = Some((request, pos, fen, sink));
                            } else {
                                begin!(request, pos, fen, sink);
                            }
                        }
                    }
                }
                Some(Cmd::Stop) => {
                    pending = None;
                    if let Some(a) = active.as_mut() {
                        if a.stopping.is_none() { let _ = process.send("stop").await; a.stopping = Some(Instant::now()); }
                    }
                }
                Some(Cmd::Configure(options, ack)) => {
                    // Options may only change while idle: finish (discard) any search first.
                    pending = None;
                    let mut result = Ok(());
                    if let Some(mut a) = active.take() {
                        a.superseded = true;
                        if a.stopping.is_none() { let _ = process.send("stop").await; }
                        let drained = tokio::time::timeout(Duration::from_secs(5), async {
                            while let Some(line) = process.next_line().await {
                                if matches!(parse_line(&line), EngineLine::BestMove { .. }) { return true; }
                            }
                            false
                        }).await;
                        if !matches!(drained, Ok(true)) { result = Err(anyhow!("engine did not stop")); }
                    }
                    if result.is_ok() {
                        for (name, value) in &options {
                            if name.eq_ignore_ascii_case("MultiPV") { continue; }
                            if let Err(e) = process.send(&setoption(name, value.as_deref())).await { result = Err(e); break; }
                        }
                    }
                    if result.is_ok() { result = process.ready(Duration::from_secs(30)).await; }
                    let failed = result.is_err();
                    let _ = ack.send(result);
                    if failed { process.kill(); return; }
                }
            },
            line = process.next_line() => match line {
                None => {
                    let message = format!("{engine} stopped unexpectedly.");
                    if let Some(a) = active.take() { let _ = a.sink.send(SearchEvent::Error { search_id: a.request.search_id, message: message.clone() }); }
                    if let Some((r, _, _, sink)) = pending.take() { let _ = sink.send(SearchEvent::Error { search_id: r.search_id, message }); }
                    return;
                }
                Some(line) => match parse_line(&line) {
                    EngineLine::Info(info) => {
                        if let Some(text) = info.string.as_deref() {
                            if text.contains("CRITICAL ERROR") || text.starts_with("ERROR") {
                                tracing::warn!("{engine}: {text}");
                                if let Some(a) = active.as_ref() {
                                    let _ = a.sink.send(SearchEvent::Error { search_id: a.request.search_id, message: format!("{engine}: {text}") });
                                }
                            }
                        }
                        if let Some(a) = active.as_mut() {
                            if !a.superseded { a.absorb(info); }
                        }
                    }
                    EngineLine::BestMove { best, .. } => {
                        if let Some(a) = active.take() {
                            if !a.superseded {
                                let snapshot = a.snapshot(&engine);
                                let best_move = best.filter(|m| chess::parse_uci(&a.pos, m).is_ok());
                                let _ = a.sink.send(SearchEvent::Done { snapshot, best_move });
                            }
                        }
                        if let Some((req, pos, fen, sink)) = pending.take() {
                            begin!(req, pos, fen, sink);
                        }
                    }
                    _ => {}
                }
            },
            _ = tick.tick() => {
                if let Some(a) = active.as_mut() {
                    if a.dirty && !a.superseded {
                        a.dirty = false;
                        let _ = a.sink.send(SearchEvent::Update { snapshot: a.snapshot(&engine) });
                    }
                    if let Some(since) = a.stopping {
                        if since.elapsed() > Duration::from_secs(8) {
                            // Hung engine: it never answered `stop`.
                            let message = format!("{engine} did not respond to stop and was terminated.");
                            let _ = a.sink.send(SearchEvent::Error { search_id: a.request.search_id, message: message.clone() });
                            if let Some((r, _, _, sink)) = pending.take() { let _ = sink.send(SearchEvent::Error { search_id: r.search_id, message }); }
                            process.kill();
                            return;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_commands() {
        assert_eq!(SearchLimit::default().go_command(), "go infinite");
        assert_eq!(SearchLimit { depth: Some(18), ..Default::default() }.go_command(), "go depth 18");
        assert_eq!(SearchLimit { movetime_ms: Some(500), nodes: Some(1000), ..Default::default() }.go_command(), "go movetime 500 nodes 1000");
    }

    #[test]
    fn position_command_normalises_fen() {
        let req = SearchRequest { search_id: 1, fen: None, moves: vec!["e2e4".into()], multipv: 1, limit: SearchLimit::default() };
        assert_eq!(position_command(&req), "position startpos moves e2e4");
        let req = SearchRequest { fen: Some("  4k3/8/8/8/8/8/8/4K2R w K - 0 1 ".into()), moves: vec![], ..req };
        assert_eq!(position_command(&req), "position fen 4k3/8/8/8/8/8/8/4K2R w K - 0 1");
    }

    #[test]
    fn prepare_rejects_illegal_and_short_circuits_terminal() {
        let bad = SearchRequest { search_id: 1, fen: None, moves: vec!["e2e5".into()], multipv: 1, limit: SearchLimit::default() };
        assert!(prepare(&bad, "x").is_err());
        let mate = SearchRequest { moves: ["f2f3", "e7e5", "g2g4", "d8h4"].map(String::from).to_vec(), ..bad };
        let (_, _, snap) = prepare(&mate, "x").unwrap();
        let snap = snap.unwrap();
        assert_eq!(snap.terminal.as_deref(), Some("checkmate"));
        // White to move and mated: Black won.
        assert_eq!(snap.lines[0].score, Score::Mate(-1));
        assert_eq!(snap.lines[0].raw_score, Score::Mate(0));
    }

    #[test]
    fn absorb_converts_to_white_pov_and_ignores_bounds() {
        let pos = chess::replay(None, &["e2e4".to_string()]).unwrap().pop().unwrap();
        let (sink, _rx) = mpsc::unbounded_channel();
        let mut a = Active {
            request: SearchRequest { search_id: 7, fen: None, moves: vec![], multipv: 2, limit: SearchLimit::default() },
            fen: chess::to_fen(&pos),
            pos,
            sink,
            lines: BTreeMap::new(),
            last: Info::default(),
            depth: 0,
            dirty: false,
            stopping: None,
            superseded: false,
        };
        let EngineLine::Info(info) = parse_line("info depth 10 multipv 1 score cp 30 wdl 50 900 50 nodes 5 pv e7e5 g1f3") else { panic!() };
        a.absorb(info);
        let EngineLine::Info(info) = parse_line("info depth 11 multipv 1 score cp 99 upperbound nodes 6 pv d7d5") else { panic!() };
        a.absorb(info);
        let snap = a.snapshot("SF");
        assert_eq!(snap.lines.len(), 1);
        assert_eq!(snap.lines[0].score, Score::Cp(-30), "black to move: +30 for black is -30 for white");
        assert_eq!(snap.lines[0].raw_score, Score::Cp(30));
        assert_eq!(snap.lines[0].san, ["e5", "Nf3"]);
        assert_eq!(snap.depth, 10);
        assert_eq!(snap.nodes, Some(6));
    }
}
