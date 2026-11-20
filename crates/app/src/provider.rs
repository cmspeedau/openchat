use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Message {
    pub role: String,
    pub content: String,
}

pub trait Provider: Send + Sync {
    fn stream(&self, messages: &[Message], on_chunk: &mut dyn FnMut(&str)) -> Result<(), String>;
}

#[derive(Default)]
pub struct MockProvider;

impl Provider for MockProvider {
    fn stream(&self, messages: &[Message], on_chunk: &mut dyn FnMut(&str)) -> Result<(), String> {
        let last = messages.iter().rev().find(|message| message.role == "user")
            .map(|message| message.content.as_str()).unwrap_or("");
        let reply = format!("Mock reply: {last}");
        let characters = reply.chars().collect::<Vec<_>>();
        for chunk in characters.chunks(8) {
            let chunk = chunk.iter().collect::<String>();
            on_chunk(&chunk);
            std::thread::sleep(Duration::from_millis(25));
        }
        Ok(())
    }
}

/// OpenAI-compatible chat-completions endpoint, enabled only with an API key.
pub struct OpenAICompatProvider {
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenAICompatProvider {
    pub fn from_env() -> Option<Self> {
        Self::from_env_with_overrides(None, None)
    }

    pub fn from_env_with_overrides(base_url: Option<&str>, model: Option<&str>) -> Option<Self> {
        let configured_base = std::env::var("OPENCHAT_BASE_URL").ok();
        let api_key = std::env::var("OPENCHAT_API_KEY").ok()?;
        let base_url = base_url.or(configured_base.as_deref())?;
        if base_url.trim().is_empty() || api_key.trim().is_empty() {
            return None;
        }
        let model = model.map(str::to_owned)
            .or_else(|| std::env::var("OPENCHAT_MODEL").ok())
            .filter(|model| !model.trim().is_empty())
            .unwrap_or_else(|| "gpt-4o-mini".into());
        Some(Self {
            base_url: base_url.trim().trim_end_matches('/').to_string(),
            api_key,
            model,
        })
    }
}

impl Provider for OpenAICompatProvider {
    fn stream(&self, messages: &[Message], on_chunk: &mut dyn FnMut(&str)) -> Result<(), String> {
        let endpoint = format!("{}/v1/chat/completions", self.base_url);
        let payload = json!({
            "model": self.model,
            "stream": true,
            "messages": messages.iter().map(|message| json!({
                "role": message.role,
                "content": message.content,
            })).collect::<Vec<Value>>(),
        });
        let response = ureq::AgentBuilder::new().timeout(Duration::from_secs(60)).build()
            .post(&endpoint)
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .set("Accept", "text/event-stream")
            .send_json(payload)
            .map_err(|error| error.to_string())?;
        let mut data = Vec::new();
        let mut done = false;
        for line in BufReader::new(response.into_reader()).lines() {
            let line = line.map_err(|error| format!("failed reading provider stream: {error}"))?;
            if line.is_empty() {
                if process_sse_data(&data, on_chunk)? {
                    done = true;
                    break;
                }
                data.clear();
            } else if let Some(value) = line.strip_prefix("data:") {
                data.push(value.strip_prefix(' ').unwrap_or(value).to_string());
            }
        }
        if !done && !data.is_empty() {
            done = process_sse_data(&data, on_chunk)?;
        }
        if done { Ok(()) } else { Err("provider stream ended before the [DONE] marker".into()) }
    }
}

/// Parse an SSE event, emitting any content delta. Returns true at [DONE].
fn process_sse_data(data: &[String], on_chunk: &mut dyn FnMut(&str)) -> Result<bool, String> {
    if data.is_empty() { return Ok(false); }
    let payload = data.join("\n");
    if payload.trim() == "[DONE]" { return Ok(true); }
    let event: Value = serde_json::from_str(&payload)
        .map_err(|error| format!("invalid provider stream event: {error}"))?;
    if let Some(error) = event.get("error") {
        return Err(format!("provider stream error: {error}"));
    }
    if let Some(content) = event.pointer("/choices/0/delta/content") {
        match content {
            Value::String(text) => if !text.is_empty() { on_chunk(text); },
            Value::Array(parts) => for part in parts {
                if let Some(text) = part.get("text").and_then(Value::as_str) { on_chunk(text); }
            },
            Value::Null => {},
            _ => return Err("provider stream returned unsupported content delta".into()),
        }
    }
    Ok(false)
}
