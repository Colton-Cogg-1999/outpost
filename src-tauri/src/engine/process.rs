//! A UCI engine subprocess. The executable path is passed directly to the OS
//! (no shell), with no arguments, so a path can never be interpreted as a command.

use super::uci::{parse_line, EngineLine, UciOption};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineIdentity {
    pub name: String,
    pub author: Option<String>,
    pub options: Vec<UciOption>,
}

pub struct Process {
    child: Child,
    stdin: ChildStdin,
    lines: mpsc::Receiver<String>,
}

const MAX_LINE: usize = 64 * 1024;

impl Process {
    pub async fn spawn(path: &Path) -> Result<Process> {
        let meta = std::fs::metadata(path).with_context(|| format!("engine not found at {}", path.display()))?;
        if !meta.is_file() {
            bail!("{} is not a file", path.display());
        }
        let mut command = Command::new(path);
        if let Some(dir) = path.parent() {
            // Lc0 and other engines look for networks next to the executable.
            command.current_dir(dir);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("could not start {}", path.display()))?;
        let stdin = child.stdin.take().context("engine has no stdin")?;
        let stdout = child.stdout.take().context("engine has no stdout")?;
        let (tx, rx) = mpsc::channel::<String>(1024);
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut buf = Vec::with_capacity(512);
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        buf.truncate(MAX_LINE);
                        let line = String::from_utf8_lossy(&buf).trim_end().to_string();
                        if tx.send(line).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Ok(Process { child, stdin, lines: rx })
    }

    pub async fn send(&mut self, command: &str) -> Result<()> {
        debug_assert!(!command.contains('\n'));
        tracing::trace!(">> {command}");
        self.stdin.write_all(command.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    pub async fn next_line(&mut self) -> Option<String> {
        self.lines.recv().await
    }

    pub async fn handshake(&mut self, timeout: Duration) -> Result<EngineIdentity> {
        self.send("uci").await?;
        let mut identity = EngineIdentity { name: String::new(), author: None, options: Vec::new() };
        let result = tokio::time::timeout(timeout, async {
            while let Some(line) = self.next_line().await {
                match parse_line(&line) {
                    EngineLine::Id { key, value } if key == "name" => identity.name = value,
                    EngineLine::Id { key, value } if key == "author" => identity.author = Some(value),
                    EngineLine::Option(option) => identity.options.push(option),
                    EngineLine::UciOk => return Ok(()),
                    _ => {}
                }
            }
            bail!("the program exited before completing the UCI handshake")
        })
        .await;
        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => bail!("no `uciok` within {}s — is this a UCI chess engine?", timeout.as_secs()),
        }
        if identity.name.is_empty() {
            identity.name = "Unnamed engine".into();
        }
        Ok(identity)
    }

    /// `isready` / `readyok`, discarding any other output in between.
    pub async fn ready(&mut self, timeout: Duration) -> Result<()> {
        self.send("isready").await?;
        let waited = tokio::time::timeout(timeout, async {
            while let Some(line) = self.next_line().await {
                if parse_line(&line) == EngineLine::ReadyOk {
                    return true;
                }
            }
            false
        })
        .await;
        match waited {
            Ok(true) => Ok(()),
            Ok(false) => bail!("engine exited while initialising"),
            Err(_) => bail!("engine did not answer `isready` within {}s", timeout.as_secs()),
        }
    }

    /// Asks the engine to quit, then makes sure it does.
    pub async fn quit(mut self) {
        let _ = self.send("stop").await;
        let _ = self.send("quit").await;
        if tokio::time::timeout(Duration::from_millis(1500), self.child.wait()).await.is_err() {
            let _ = self.child.kill().await;
        }
    }

    pub fn kill(&mut self) {
        let _ = self.child.start_kill();
    }
}

/// Starts an executable, performs the UCI handshake and quits. Used to validate a
/// user-selected engine before it is registered.
pub async fn probe(path: &Path) -> Result<EngineIdentity> {
    let mut process = Process::spawn(path).await?;
    let identity = process.handshake(Duration::from_secs(10)).await;
    process.quit().await;
    identity
}
