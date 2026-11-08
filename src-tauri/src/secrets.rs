//! API keys live in the operating system's credential store (Secret Service on Linux,
//! Keychain on macOS, Credential Manager on Windows). They are never written to the
//! database, settings, or logs. If no store is available, a key can still be supplied
//! through the provider's usual environment variable.

use anyhow::{anyhow, Result};
use serde::Serialize;

const SERVICE: &str = "io.github.outpost-chess";

fn entry(provider: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, &format!("api-key:{provider}")).map_err(|e| anyhow!("Credential store unavailable: {e}"))
}

pub fn env_var(provider: &str) -> Option<&'static str> {
    match provider {
        "openai" => Some("OPENAI_API_KEY"),
        "anthropic" => Some("ANTHROPIC_API_KEY"),
        "gemini" => Some("GEMINI_API_KEY"),
        "compatible" => Some("OUTPOST_COMPATIBLE_API_KEY"),
        _ => None,
    }
}

pub fn set(provider: &str, key: &str) -> Result<()> {
    let key = key.trim();
    if key.is_empty() || key.len() > 4096 || key.chars().any(char::is_control) {
        return Err(anyhow!("That does not look like an API key."));
    }
    entry(provider)?.set_password(key).map_err(|e| anyhow!("Could not save to the system credential store: {e}"))
}

pub fn delete(provider: &str) -> Result<()> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(anyhow!("Could not remove the key: {e}")),
    }
}

/// The key to use, preferring the credential store over the environment.
pub fn get(provider: &str) -> Option<String> {
    if let Ok(entry) = entry(provider) {
        if let Ok(key) = entry.get_password() {
            return Some(key);
        }
    }
    env_var(provider).and_then(|v| std::env::var(v).ok()).filter(|v| !v.trim().is_empty())
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatus {
    /// "keyring" | "environment" | "none"
    pub source: &'static str,
    pub store_available: bool,
}

pub fn status(provider: &str) -> KeyStatus {
    let (store_available, stored) = match entry(provider) {
        Ok(e) => match e.get_password() {
            Ok(_) => (true, true),
            Err(keyring::Error::NoEntry) => (true, false),
            Err(_) => (false, false),
        },
        Err(_) => (false, false),
    };
    let env = env_var(provider).and_then(|v| std::env::var(v).ok()).is_some_and(|v| !v.trim().is_empty());
    KeyStatus { source: if stored { "keyring" } else if env { "environment" } else { "none" }, store_available }
}
