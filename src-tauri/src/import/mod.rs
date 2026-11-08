//! Account imports from public, documented APIs. Requests are strictly serial, carry an
//! identifying User-Agent, honour `Retry-After`/429 and back off on server errors.
//! No passwords or tokens are ever requested.

pub mod chesscom;
pub mod lichess;

use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub fn user_agent() -> String {
    format!("OUTPOST/{} (open-source local chess analysis; +https://github.com/Colton-Cogg-1999/outpost)", env!("CARGO_PKG_VERSION"))
}

pub fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(user_agent())
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .https_only(true)
        .build()?)
}

pub async fn sleep_cancellable(duration: Duration, cancel: &AtomicBool) -> Result<()> {
    let deadline = tokio::time::Instant::now() + duration;
    while tokio::time::Instant::now() < deadline {
        if cancel.load(Ordering::Relaxed) {
            bail!("Cancelled.");
        }
        tokio::time::sleep(Duration::from_millis(200).min(deadline - tokio::time::Instant::now())).await;
    }
    Ok(())
}

#[derive(Debug)]
pub struct NotFound;

impl std::fmt::Display for NotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "not found")
    }
}

impl std::error::Error for NotFound {}

/// GET with polite retry. `rate_limit_wait` is how long to pause after a 429 when the
/// server does not say (Lichess asks clients to wait a full minute).
pub async fn get(
    client: &reqwest::Client,
    url: &str,
    accept: &str,
    rate_limit_wait: Duration,
    cancel: &AtomicBool,
    mut on_wait: impl FnMut(&str),
) -> Result<reqwest::Response> {
    let mut rate_limited = 0;
    for attempt in 0..5u32 {
        if cancel.load(Ordering::Relaxed) {
            bail!("Cancelled.");
        }
        let response = match client.get(url).header(reqwest::header::ACCEPT, accept).send().await {
            Ok(r) => r,
            Err(e) if attempt < 3 && (e.is_timeout() || e.is_connect()) => {
                on_wait("Network problem, retrying…");
                sleep_cancellable(Duration::from_secs(2u64.pow(attempt + 1)), cancel).await?;
                continue;
            }
            Err(e) => return Err(anyhow!("Could not reach {}: {e}", host(url))),
        };
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        match status.as_u16() {
            404 | 410 => return Err(NotFound.into()),
            429 => {
                rate_limited += 1;
                if rate_limited > 2 {
                    bail!("{} is rate limiting requests. Try again in a few minutes.", host(url));
                }
                let wait = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(Duration::from_secs)
                    .unwrap_or(rate_limit_wait)
                    .min(Duration::from_secs(300));
                on_wait(&format!("Rate limited by {}; waiting {}s as requested…", host(url), wait.as_secs()));
                sleep_cancellable(wait, cancel).await?;
            }
            500..=599 if attempt < 3 => {
                on_wait(&format!("{} returned {status}; retrying…", host(url)));
                sleep_cancellable(Duration::from_secs(2u64.pow(attempt + 1)), cancel).await?;
            }
            _ => bail!("{} answered {status}.", host(url)),
        }
    }
    bail!("Giving up on {} after several attempts.", host(url))
}

fn host(url: &str) -> String {
    url.split("//").nth(1).and_then(|r| r.split('/').next()).unwrap_or(url).to_string()
}

#[derive(Clone, Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    pub message: String,
    pub scanned: usize,
    pub imported: usize,
    pub duplicates: usize,
    pub skipped: usize,
    pub done: bool,
}

/// Usernames go into URL paths; only allow the characters both sites permit.
pub fn valid_username(name: &str) -> Result<String> {
    let name = name.trim();
    if name.len() < 2 || name.len() > 30 || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        bail!("'{name}' is not a valid username.");
    }
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usernames_are_restricted() {
        assert!(valid_username("Magnus_C-1").is_ok());
        assert!(valid_username("../etc").is_err());
        assert!(valid_username("a b").is_err());
        assert!(valid_username("x?y=1").is_err());
        assert!(valid_username("").is_err());
    }

    #[test]
    fn host_extraction() {
        assert_eq!(host("https://lichess.org/api/x"), "lichess.org");
    }
}
