//! Subscription access through the official CLIs, adapted from OmaReader
//! (same author, MIT). The prompt carries untrusted text (PGN comments, player names),
//! so both CLIs run with every tool disabled, in an empty working directory.

use super::{find_binary, AiEvent, ModelOption, ProviderStatus, Turn};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;

/// Variables that would make a nested `claude` behave like a child of another session.
fn clean_claude_env(command: &mut Command) {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("CLAUDE") {
            command.env_remove(key);
        }
    }
}

pub async fn claude(cwd: &Path, turn: &Turn, tx: &mpsc::UnboundedSender<AiEvent>) -> Result<()> {
    let binary = find_binary("claude").context("Claude Code isn't installed.")?;
    let mut command = Command::new(binary);
    command
        .args(["-p", "--output-format", "stream-json", "--include-partial-messages", "--verbose"])
        .args(["--no-session-persistence", "--tools", "", "--strict-mcp-config", "--setting-sources", ""])
        .arg("--disable-slash-commands")
        .args(["--system-prompt", &turn.system]);
    if let Some(model) = turn.model.as_deref().filter(|m| !m.is_empty()) {
        command.args(["--model", model]);
    }
    clean_claude_env(&mut command);
    let mut child = command
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("could not start Claude Code")?;
    let mut stdin = child.stdin.take().context("no stdin")?;
    let prompt = turn.flattened();
    tokio::spawn(async move {
        let _ = stdin.write_all(prompt.as_bytes()).await;
        let _ = stdin.shutdown().await;
    });
    let mut stderr = child.stderr.take().context("no stderr")?;
    let stderr_task = tokio::spawn(async move {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text).await;
        text
    });
    let mut lines = BufReader::new(child.stdout.take().context("no stdout")?).lines();
    let mut failure: Option<String> = None;
    let mut saw_text = false;
    while let Some(line) = lines.next_line().await? {
        let Ok(event) = serde_json::from_str::<Value>(&line) else { continue };
        match event["type"].as_str() {
            Some("stream_event") => {
                let delta = &event["event"]["delta"];
                if delta["type"] == "text_delta" {
                    if let Some(text) = delta["text"].as_str() {
                        saw_text = true;
                        if tx.send(AiEvent::Delta(text.to_string())).is_err() {
                            return Ok(());
                        }
                    }
                }
            }
            Some("result") if event["is_error"].as_bool() == Some(true) => {
                failure = Some(event["result"].as_str().unwrap_or("Claude Code reported an error.").to_string());
            }
            _ => {}
        }
    }
    let status = child.wait().await?;
    let stderr_text = stderr_task.await.unwrap_or_default();
    if let Some(message) = failure {
        return Err(anyhow!(claude_friendly(&message)));
    }
    if !status.success() {
        bail!("{}", claude_friendly(stderr_text.trim()));
    }
    if !saw_text {
        bail!("Claude Code returned an empty answer.");
    }
    Ok(())
}

fn claude_friendly(message: &str) -> String {
    let lower = message.to_lowercase();
    if lower.contains("not logged in") || lower.contains("/login") || lower.contains("invalid api key") {
        return "Claude Code isn't signed in. Run `claude` in a terminal and log in, then try again.".into();
    }
    if lower.contains("usage limit") || lower.contains("limit reached") || lower.contains("rate limit") {
        return format!("Your Claude plan hit its usage limit. {message}");
    }
    if message.is_empty() { "Claude Code stopped unexpectedly.".into() } else { message.to_string() }
}

pub async fn claude_status(cwd: &Path) -> ProviderStatus {
    let models = [("", "Default"), ("haiku", "Haiku (fastest)"), ("sonnet", "Sonnet"), ("opus", "Opus (deepest)")]
        .map(|(id, label)| ModelOption { id: id.into(), label: label.into() })
        .to_vec();
    let mut out = ProviderStatus { id: "claude".into(), ready: false, detail: "Install Claude Code, then run `claude` once to log in.".into(), plan: None, models };
    let Some(binary) = find_binary("claude") else { return out };
    let mut command = Command::new(binary);
    command.args(["auth", "status"]).current_dir(cwd).stdin(Stdio::null()).kill_on_drop(true);
    clean_claude_env(&mut command);
    if let Ok(Ok(output)) = tokio::time::timeout(Duration::from_secs(10), command.output()).await {
        if let Ok(info) = serde_json::from_slice::<Value>(&output.stdout) {
            out.ready = info["loggedIn"].as_bool().unwrap_or(false);
            out.plan = info["subscriptionType"].as_str().map(str::to_string);
        }
    }
    out.detail = if out.ready { "Signed in.".into() } else { "Installed, but not signed in. Run `claude` in a terminal and log in.".into() };
    out
}

struct Codex {
    _child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    next_id: u64,
}

impl Codex {
    async fn start(cwd: &Path) -> Result<Self> {
        let binary = find_binary("codex").context("Codex isn't installed.")?;
        let mut child = Command::new(binary)
            .arg("app-server")
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("could not start Codex")?;
        let stdin = child.stdin.take().context("no stdin")?;
        let stdout = BufReader::new(child.stdout.take().context("no stdout")?).lines();
        let mut client = Codex { _child: child, stdin, stdout, next_id: 0 };
        client
            .request(
                "initialize",
                json!({"clientInfo": {"name": "outpost", "title": "OUTPOST", "version": env!("CARGO_PKG_VERSION")}, "capabilities": {"experimentalApi": true}}),
            )
            .await?;
        client.write(&json!({"method": "initialized"})).await?;
        Ok(client)
    }

    async fn write(&mut self, message: &Value) -> Result<()> {
        let mut line = message.to_string();
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await?;
        Ok(())
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        self.write(&json!({"id": id, "method": method, "params": params})).await?;
        loop {
            let line = self.stdout.next_line().await?.context("Codex closed unexpectedly")?;
            let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
            if message.get("method").is_none() && message["id"].as_u64() == Some(id) {
                if let Some(error) = message.get("error") {
                    bail!("{}", error["message"].as_str().unwrap_or("Codex reported an error."));
                }
                return Ok(message["result"].clone());
            }
        }
    }
}

/// Everything except talking is off: no shell, patch, web, browser or sub-agent tools,
/// and a read-only sandbox without network.
fn codex_locked_config() -> Value {
    let off: HashMap<&str, bool> = [
        "apps", "browser_use", "code_mode_host", "computer_use", "goals", "image_generation", "in_app_browser",
        "multi_agent", "multi_agent_v2", "plugins", "remote_plugin", "shell_snapshot", "shell_tool", "unified_exec",
    ]
    .into_iter()
    .map(|name| (name, false))
    .collect();
    json!({"features": off, "web_search": "disabled", "shell_environment_policy": {"inherit": "none"}})
}

pub async fn codex(cwd: &Path, turn: &Turn, tx: &mpsc::UnboundedSender<AiEvent>) -> Result<()> {
    let mut client = Codex::start(cwd).await?;
    let mut params = json!({
        "cwd": cwd,
        "ephemeral": true,
        "sandbox": "read-only",
        "approvalPolicy": "never",
        "baseInstructions": turn.system,
        "config": codex_locked_config(),
        "serviceName": "outpost",
    });
    if let Some(model) = turn.model.as_deref().filter(|m| !m.is_empty()) {
        params["model"] = json!(model);
    }
    let thread = client.request("thread/start", params).await?;
    let thread_id = thread["thread"]["id"].as_str().context("Codex returned no thread")?.to_string();
    client
        .request("turn/start", json!({"threadId": thread_id, "effort": "low", "input": [{"type": "text", "text": turn.flattened()}]}))
        .await?;
    let mut saw_text = false;
    while let Some(line) = client.stdout.next_line().await? {
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        match message["method"].as_str() {
            Some("item/agentMessage/delta") => {
                if let Some(delta) = message["params"]["delta"].as_str() {
                    saw_text = true;
                    if tx.send(AiEvent::Delta(delta.to_string())).is_err() {
                        return Ok(());
                    }
                }
            }
            Some("item/started") => {
                let kind = message["params"]["item"]["type"].as_str().unwrap_or("");
                if !matches!(kind, "userMessage" | "agentMessage" | "reasoning" | "plan" | "contextCompaction" | "hookPrompt") {
                    bail!("Stopped: Codex tried to use a tool ({kind}), which OUTPOST doesn't allow.");
                }
            }
            Some("turn/completed") => {
                let turn = &message["params"]["turn"];
                if turn["status"] == "failed" {
                    bail!("{}", codex_friendly(turn["error"]["message"].as_str().unwrap_or("Codex could not finish the answer.")));
                }
                if !saw_text {
                    bail!("Codex returned an empty answer.");
                }
                return Ok(());
            }
            Some("error") => {
                if message["params"]["willRetry"].as_bool() != Some(true) {
                    bail!("{}", codex_friendly(message["params"]["error"]["message"].as_str().unwrap_or("Codex reported an error.")));
                }
            }
            _ => {
                // Server-initiated requests carry an id; decline anything asking for approval.
                if let (Some(id), Some(_)) = (message.get("id"), message.get("method")) {
                    client.write(&json!({"id": id, "result": {"decision": "decline"}})).await?;
                }
            }
        }
    }
    Err(anyhow!("Codex closed before finishing the answer."))
}

fn codex_friendly(message: &str) -> String {
    let lower = message.to_lowercase();
    if lower.contains("login") || lower.contains("not authenticated") || lower.contains("unauthorized") {
        return "Codex isn't signed in. Run `codex login` in a terminal, then try again.".into();
    }
    message.to_string()
}

async fn codex_probe(cwd: &Path) -> Result<(bool, Option<String>, Vec<ModelOption>)> {
    let mut client = Codex::start(cwd).await?;
    let account = client.request("account/read", json!({})).await?;
    let signed_in = account["account"].is_object();
    let plan = account["account"]["planType"].as_str().map(str::to_string);
    let list = client.request("model/list", json!({})).await?;
    let mut models = vec![ModelOption { id: String::new(), label: "Default".into() }];
    for item in list["data"].as_array().into_iter().flatten() {
        let id = item["id"].as_str().or(item["model"].as_str()).unwrap_or("").to_string();
        let label = item["displayName"].as_str().unwrap_or(&id).to_string();
        if !id.is_empty() {
            models.push(ModelOption { id, label });
        }
    }
    Ok((signed_in, plan, models))
}

pub async fn codex_status(cwd: &Path) -> ProviderStatus {
    let mut out = ProviderStatus {
        id: "codex".into(),
        ready: false,
        detail: "Install the Codex CLI, then run `codex login` with your ChatGPT account.".into(),
        plan: None,
        models: vec![ModelOption { id: String::new(), label: "Default".into() }],
    };
    if find_binary("codex").is_none() {
        return out;
    }
    match tokio::time::timeout(Duration::from_secs(12), codex_probe(cwd)).await {
        Ok(Ok((signed_in, plan, models))) => {
            out.ready = signed_in;
            out.plan = plan;
            out.models = models;
            out.detail = if signed_in { "Signed in.".into() } else { "Installed, but not signed in. Run `codex login` in a terminal.".into() };
        }
        _ => out.detail = "Codex didn't respond. Try running `codex` in a terminal.".into(),
    }
    out
}
