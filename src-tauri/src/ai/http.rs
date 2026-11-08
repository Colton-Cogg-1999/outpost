//! HTTP providers. All streaming formats reduce to "lines": Server-Sent Events
//! (`data: {...}`) for OpenAI-style, Anthropic and Gemini, newline-delimited JSON for Ollama.

use super::{check_base_url, provider, AiEvent, Message, ModelOption, ProviderConfig, ProviderStatus, Turn};
use crate::secrets;
use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::mpsc;

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(crate::import::user_agent())
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(180))
        .build()?)
}

fn base_url(id: &str, config: &ProviderConfig) -> Result<String> {
    let default = provider(id).and_then(|p| p.default_base_url).unwrap_or_default();
    check_base_url(config.base_url.as_deref().filter(|u| !u.trim().is_empty()).unwrap_or(default))
}

fn key(id: &str, required: bool) -> Result<Option<String>> {
    let key = secrets::get(id);
    if required && key.is_none() {
        let name = provider(id).map(|p| p.name).unwrap_or(id);
        bail!("No API key for {name}. Add one in AI settings{}.", secrets::env_var(id).map(|v| format!(" or set {v}")).unwrap_or_default());
    }
    Ok(key)
}

/// Calls `f` with each complete line of the response body; stops when `f` returns false.
async fn each_line(response: reqwest::Response, mut f: impl FnMut(&str) -> Result<bool>) -> Result<()> {
    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        buffer.extend_from_slice(&chunk?);
        while let Some(pos) = buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = buffer.drain(..=pos).collect();
            let text = String::from_utf8_lossy(&line);
            if !f(text.trim_end_matches(['\r', '\n']))? {
                return Ok(());
            }
        }
        if buffer.len() > 8 * 1024 * 1024 {
            bail!("Unexpectedly long line from the AI provider.");
        }
    }
    if !buffer.is_empty() {
        let text = String::from_utf8_lossy(&buffer).to_string();
        f(text.trim())?;
    }
    Ok(())
}

async fn check(response: reqwest::Response, name: &str) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    let detail = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().or(v["error"].as_str()).or(v["message"].as_str()).map(str::to_string))
        .unwrap_or_else(|| body.chars().take(300).collect());
    Err(match status.as_u16() {
        401 | 403 => anyhow!("{name} rejected the credentials ({status}). {detail}"),
        404 => anyhow!("{name}: not found ({status}). Check the model name and server URL. {detail}"),
        429 => anyhow!("{name} is rate limiting or out of quota ({status}). {detail}"),
        _ => anyhow!("{name} returned {status}. {detail}"),
    })
}

fn openai_messages(turn: &Turn) -> Vec<Value> {
    let mut out = vec![json!({"role": "system", "content": turn.system})];
    out.extend(turn.messages.iter().map(|m| json!({"role": m.role, "content": m.content})));
    out
}

fn require_model(turn: &Turn, name: &str) -> Result<String> {
    turn.model.clone().filter(|m| !m.trim().is_empty()).ok_or_else(|| anyhow!("Choose a model for {name} in AI settings."))
}

pub async fn run(id: &str, config: &ProviderConfig, turn: &Turn, tx: &mpsc::UnboundedSender<AiEvent>) -> Result<()> {
    let client = client()?;
    let base = base_url(id, config)?;
    let name = provider(id).map(|p| p.name).unwrap_or(id);
    let mut saw_text = false;
    let mut send = |text: &str| -> bool {
        if text.is_empty() {
            return true;
        }
        saw_text = true;
        tx.send(AiEvent::Delta(text.to_string())).is_ok()
    };
    match id {
        "anthropic" => {
            let key = key(id, true)?.unwrap();
            let model = require_model(turn, name)?;
            let messages: Vec<Value> = turn.messages.iter().map(|m| json!({"role": m.role, "content": m.content})).collect();
            let response = client
                .post(format!("{base}/v1/messages"))
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01")
                .json(&json!({"model": model, "max_tokens": 2048, "system": turn.system, "messages": messages, "stream": true}))
                .send()
                .await
                .with_context(|| format!("Could not reach {name}"))?;
            let response = check(response, name).await?;
            let mut error: Option<String> = None;
            each_line(response, |line| {
                let Some(data) = line.strip_prefix("data:") else { return Ok(true) };
                let Ok(event) = serde_json::from_str::<Value>(data.trim()) else { return Ok(true) };
                match event["type"].as_str() {
                    Some("content_block_delta") if event["delta"]["type"] == "text_delta" => Ok(send(event["delta"]["text"].as_str().unwrap_or(""))),
                    Some("error") => {
                        error = Some(event["error"]["message"].as_str().unwrap_or("stream error").to_string());
                        Ok(false)
                    }
                    Some("message_stop") => Ok(false),
                    _ => Ok(true),
                }
            })
            .await?;
            if let Some(e) = error {
                bail!("{name}: {e}");
            }
        }
        "openai" | "compatible" => {
            let key = key(id, id == "openai")?;
            let model = require_model(turn, name)?;
            let mut request = client.post(format!("{base}/chat/completions")).json(&json!({"model": model, "messages": openai_messages(turn), "stream": true}));
            if let Some(key) = key {
                request = request.bearer_auth(key);
            }
            let response = check(request.send().await.with_context(|| format!("Could not reach {name} at {base}"))?, name).await?;
            each_line(response, |line| {
                let Some(data) = line.strip_prefix("data:").map(str::trim) else { return Ok(true) };
                if data == "[DONE]" {
                    return Ok(false);
                }
                let Ok(event) = serde_json::from_str::<Value>(data) else { return Ok(true) };
                if let Some(message) = event["error"]["message"].as_str() {
                    bail!("{name}: {message}");
                }
                Ok(send(event["choices"][0]["delta"]["content"].as_str().unwrap_or("")))
            })
            .await?;
        }
        "gemini" => {
            let key = key(id, true)?.unwrap();
            let model = require_model(turn, name)?;
            let model = model.strip_prefix("models/").unwrap_or(&model).to_string();
            if !model.chars().all(|c| c.is_ascii_alphanumeric() || "-._".contains(c)) {
                bail!("Invalid Gemini model name.");
            }
            let contents: Vec<Value> = turn
                .messages
                .iter()
                .map(|m| json!({"role": if m.role == "assistant" { "model" } else { "user" }, "parts": [{"text": m.content}]}))
                .collect();
            let response = client
                .post(format!("{base}/v1beta/models/{model}:streamGenerateContent?alt=sse"))
                .header("x-goog-api-key", key)
                .json(&json!({"systemInstruction": {"parts": [{"text": turn.system}]}, "contents": contents}))
                .send()
                .await
                .with_context(|| format!("Could not reach {name}"))?;
            let response = check(response, name).await?;
            each_line(response, |line| {
                let Some(data) = line.strip_prefix("data:") else { return Ok(true) };
                let Ok(event) = serde_json::from_str::<Value>(data.trim()) else { return Ok(true) };
                if let Some(message) = event["error"]["message"].as_str() {
                    bail!("{name}: {message}");
                }
                let mut keep = true;
                for part in event["candidates"][0]["content"]["parts"].as_array().into_iter().flatten() {
                    keep &= send(part["text"].as_str().unwrap_or(""));
                }
                Ok(keep)
            })
            .await?;
        }
        "ollama" => {
            let model = require_model(turn, name)?;
            let response = client
                .post(format!("{base}/api/chat"))
                .json(&json!({"model": model, "messages": openai_messages(turn), "stream": true}))
                .send()
                .await
                .with_context(|| format!("Could not reach Ollama at {base}. Is it running?"))?;
            let response = check(response, name).await?;
            each_line(response, |line| {
                let Ok(event) = serde_json::from_str::<Value>(line.trim()) else { return Ok(true) };
                if let Some(message) = event["error"].as_str() {
                    bail!("Ollama: {message}");
                }
                let keep = send(event["message"]["content"].as_str().unwrap_or(""));
                Ok(keep && event["done"].as_bool() != Some(true))
            })
            .await?;
        }
        other => bail!("Unknown provider {other}"),
    }
    if !saw_text {
        bail!("{name} returned an empty answer.");
    }
    Ok(())
}

pub async fn list_models(id: &str, config: &ProviderConfig) -> Result<Vec<ModelOption>> {
    let client = client()?;
    let base = base_url(id, config)?;
    let name = provider(id).map(|p| p.name).unwrap_or(id);
    let request = match id {
        "anthropic" => client
            .get(format!("{base}/v1/models?limit=100"))
            .header("x-api-key", key(id, true)?.unwrap())
            .header("anthropic-version", "2023-06-01"),
        "openai" | "compatible" => {
            let mut r = client.get(format!("{base}/models"));
            if let Some(key) = key(id, id == "openai")? {
                r = r.bearer_auth(key);
            }
            r
        }
        "gemini" => client.get(format!("{base}/v1beta/models?pageSize=200")).header("x-goog-api-key", key(id, true)?.unwrap()),
        "ollama" => client.get(format!("{base}/api/tags")),
        other => bail!("Unknown provider {other}"),
    };
    let response = check(request.timeout(Duration::from_secs(20)).send().await.with_context(|| format!("Could not reach {name} at {base}"))?, name).await?;
    let body: Value = response.json().await?;
    let mut models: Vec<ModelOption> = match id {
        "ollama" => body["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| m["name"].as_str())
            .map(|n| ModelOption { id: n.into(), label: n.into() })
            .collect(),
        "gemini" => body["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["supportedGenerationMethods"].as_array().is_some_and(|a| a.iter().any(|x| x == "generateContent")))
            .filter_map(|m| {
                let id = m["name"].as_str()?.strip_prefix("models/")?.to_string();
                Some(ModelOption { label: m["displayName"].as_str().unwrap_or(&id).to_string(), id })
            })
            .collect(),
        _ => body["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| {
                let id = m["id"].as_str()?.to_string();
                Some(ModelOption { label: m["display_name"].as_str().unwrap_or(&id).to_string(), id })
            })
            .collect(),
    };
    if id == "openai" {
        // The list includes embeddings, audio, image models etc.; keep chat-capable families.
        models.retain(|m| (m.id.starts_with("gpt-") || m.id.starts_with('o') || m.id.starts_with("chatgpt")) && !["audio", "realtime", "tts", "transcribe", "image", "embedding", "search"].iter().any(|x| m.id.contains(x)));
    }
    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(models)
}

pub async fn status(id: &str, config: &ProviderConfig) -> ProviderStatus {
    let info = provider(id);
    let mut out = ProviderStatus { id: id.into(), ready: false, detail: String::new(), plan: None, models: vec![] };
    if info.is_none() {
        out.detail = "Unknown provider.".into();
        return out;
    }
    if info.is_some_and(|i| i.needs_key) && secrets::get(id).is_none() {
        out.detail = "No API key yet.".into();
        return out;
    }
    match list_models(id, config).await {
        Ok(models) => {
            out.ready = true;
            out.detail = format!("{} model(s) available.", models.len());
            out.models = models;
        }
        Err(e) => out.detail = format!("{e:#}"),
    }
    out
}

pub fn history(messages: &[Message], limit: usize) -> Vec<Message> {
    let start = messages.len().saturating_sub(limit);
    messages[start..].to_vec()
}
