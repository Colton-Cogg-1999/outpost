//! Optional AI explanations, provider-agnostic.
//!
//! Two families of provider, following OmaReader's design:
//! * **Subscription CLIs** (Claude Code, Codex): OUTPOST runs the official CLI the user
//!   has already signed in to. OUTPOST never sees, reads or stores those credentials,
//!   and runs the CLI with every tool disabled in an empty working directory.
//! * **HTTP APIs** (OpenAI, Anthropic, Gemini, any OpenAI-compatible server, Ollama):
//!   keys come from the OS credential store or environment; local servers need none.
//!
//! The chess engine and rules library stay authoritative: prompts carry validated
//! facts, and every move in an answer is re-validated before it can be played.

pub mod cli;
pub mod context;
pub mod http;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

pub struct Turn {
    pub system: String,
    pub messages: Vec<Message>,
    pub model: Option<String>,
}

impl Turn {
    /// Single-prompt form for CLIs: history rendered as a transcript.
    pub fn flattened(&self) -> String {
        let mut out = String::new();
        let (last, history) = self.messages.split_last().map(|(l, h)| (Some(l), h)).unwrap_or((None, &[]));
        if !history.is_empty() {
            out.push_str("Earlier in this conversation:\n");
            for m in history {
                out.push_str(&format!("[{}]\n{}\n\n", m.role, m.content));
            }
        }
        if let Some(last) = last {
            out.push_str(&last.content);
        }
        out
    }
}

#[derive(Debug)]
pub enum AiEvent {
    Delta(String),
    Failed(String),
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderConfig {
    pub model: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AiSettings {
    /// Selected provider id, or None when AI is not set up.
    pub active: Option<String>,
    pub providers: BTreeMap<String, ProviderConfig>,
    pub skill_level: String,
    pub save_chats: bool,
    /// "short" | "normal" | "detailed"
    pub length: String,
}

impl Default for AiSettings {
    fn default() -> Self {
        AiSettings { active: None, providers: BTreeMap::new(), skill_level: "club player (~1500)".into(), save_chats: false, length: "normal".into() }
    }
}

impl AiSettings {
    pub fn config(&self, id: &str) -> ProviderConfig {
        self.providers.get(id).cloned().unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: &'static str,
    pub name: &'static str,
    /// "cli" | "api" | "local"
    pub kind: &'static str,
    pub needs_key: bool,
    pub default_base_url: Option<&'static str>,
    pub description: &'static str,
}

pub const PROVIDERS: &[ProviderInfo] = &[
    ProviderInfo { id: "claude", name: "Claude Code", kind: "cli", needs_key: false, default_base_url: None, description: "Uses your Claude subscription through the official Claude Code CLI you are signed in to." },
    ProviderInfo { id: "codex", name: "Codex (ChatGPT)", kind: "cli", needs_key: false, default_base_url: None, description: "Uses your ChatGPT plan through the official Codex CLI (`codex login`). A ChatGPT plan is not OpenAI API access; this is the supported route." },
    ProviderInfo { id: "anthropic", name: "Anthropic API", kind: "api", needs_key: true, default_base_url: Some("https://api.anthropic.com"), description: "Claude models with an API key from console.anthropic.com (billed per use)." },
    ProviderInfo { id: "openai", name: "OpenAI API", kind: "api", needs_key: true, default_base_url: Some("https://api.openai.com/v1"), description: "OpenAI models with an API key from platform.openai.com (billed per use)." },
    ProviderInfo { id: "gemini", name: "Google Gemini API", kind: "api", needs_key: true, default_base_url: Some("https://generativelanguage.googleapis.com"), description: "Gemini models with a Google AI Studio API key." },
    ProviderInfo { id: "ollama", name: "Ollama (local)", kind: "local", needs_key: false, default_base_url: Some("http://127.0.0.1:11434"), description: "Models running on this computer with Ollama. Nothing leaves your machine." },
    ProviderInfo { id: "compatible", name: "OpenAI-compatible server", kind: "local", needs_key: false, default_base_url: Some("http://127.0.0.1:1234/v1"), description: "LM Studio, llama.cpp, vLLM, or any server speaking the OpenAI chat API. Key optional." },
];

pub fn provider(id: &str) -> Option<&'static ProviderInfo> {
    PROVIDERS.iter().find(|p| p.id == id)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub id: String,
    pub ready: bool,
    pub detail: String,
    pub plan: Option<String>,
    pub models: Vec<ModelOption>,
}

/// Desktop launchers often get a smaller PATH than a shell, so per-user install
/// locations are checked too (same list OmaReader uses).
pub fn find_binary(name: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    dirs.push(home.join(format!(".local/share/mise/installs/{name}/latest")));
    dirs.push(home.join(format!(".local/share/mise/installs/{name}/latest/bin")));
    dirs.push(home.join(".local/share/mise/shims"));
    dirs.push(home.join(".local/bin"));
    dirs.push(home.join(".npm-global/bin"));
    dirs.push(home.join(".claude/local"));
    dirs.push(home.join(".bun/bin"));
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs.into_iter().map(|d| d.join(name)).find(|p| crate::engine::discover::is_executable(p))
}

/// Base URLs are user-configurable. Plain HTTP is allowed only for this machine or a
/// private network, so an API key is never sent in clear text across the internet.
pub fn check_base_url(url: &str) -> Result<String> {
    let url = url.trim().trim_end_matches('/').to_string();
    let parsed = reqwest::Url::parse(&url).map_err(|_| anyhow::anyhow!("'{url}' is not a valid URL."))?;
    match parsed.scheme() {
        "https" => Ok(url),
        "http" => {
            let host = parsed.host_str().unwrap_or("");
            let private = host == "localhost"
                || host.ends_with(".local")
                || host.parse::<std::net::IpAddr>().is_ok_and(|ip| match ip {
                    std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
                    std::net::IpAddr::V6(v6) => v6.is_loopback(),
                })
                || host == "[::1]";
            if private {
                Ok(url)
            } else {
                bail!("Use https:// for remote servers (plain http is only allowed on this machine or a private network).")
            }
        }
        _ => bail!("Only http(s) URLs are supported."),
    }
}

pub async fn run(id: &str, settings: &AiSettings, cwd: &Path, turn: Turn, tx: mpsc::UnboundedSender<AiEvent>) {
    let config = settings.config(id);
    let result = match id {
        "claude" => cli::claude(cwd, &turn, &tx).await,
        "codex" => cli::codex(cwd, &turn, &tx).await,
        "anthropic" | "openai" | "gemini" | "ollama" | "compatible" => http::run(id, &config, &turn, &tx).await,
        other => Err(anyhow::anyhow!("Unknown AI provider '{other}'.")),
    };
    if let Err(error) = result {
        let _ = tx.send(AiEvent::Failed(format!("{error:#}")));
    }
}

pub async fn status(id: &str, settings: &AiSettings, cwd: &Path) -> ProviderStatus {
    match id {
        "claude" => cli::claude_status(cwd).await,
        "codex" => cli::codex_status(cwd).await,
        _ => http::status(id, &settings.config(id)).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_policy() {
        assert!(check_base_url("https://api.example.com/v1/").unwrap().ends_with("/v1"));
        assert!(check_base_url("http://127.0.0.1:11434").is_ok());
        assert!(check_base_url("http://localhost:1234/v1").is_ok());
        assert!(check_base_url("http://192.168.1.20:8080").is_ok());
        assert!(check_base_url("http://api.example.com").is_err());
        assert!(check_base_url("file:///etc/passwd").is_err());
        assert!(check_base_url("not a url").is_err());
    }

    #[test]
    fn flattened_transcript() {
        let turn = Turn {
            system: String::new(),
            messages: vec![
                Message { role: "user".into(), content: "Q1".into() },
                Message { role: "assistant".into(), content: "A1".into() },
                Message { role: "user".into(), content: "Q2".into() },
            ],
            model: None,
        };
        let text = turn.flattened();
        assert!(text.starts_with("Earlier in this conversation:"));
        assert!(text.ends_with("Q2"));
    }
}
