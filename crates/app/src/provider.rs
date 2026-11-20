use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Message {
    pub role: String,
    pub content: String,
}

pub trait Provider: Send + Sync {
    fn send(&self, messages: &[Message]) -> Result<String, String>;
}

#[derive(Default)]
pub struct MockProvider;

impl Provider for MockProvider {
    fn send(&self, messages: &[Message]) -> Result<String, String> {
        let last = messages.iter().rev().find(|message| message.role == "user")
            .map(|message| message.content.as_str()).unwrap_or("");
        Ok(format!("Mock reply: {last}"))
    }
}

/// OpenAI-compatible chat-completions endpoint, enabled only with both environment values.
pub struct OpenAICompatProvider {
    base_url: String,
    api_key: String,
}

impl OpenAICompatProvider {
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("OPENCHAT_BASE_URL").ok()?;
        let api_key = std::env::var("OPENCHAT_API_KEY").ok()?;
        if base_url.trim().is_empty() || api_key.trim().is_empty() {
            return None;
        }
        Some(Self { base_url: base_url.trim_end_matches('/').to_string(), api_key })
    }
}

impl Provider for OpenAICompatProvider {
    fn send(&self, messages: &[Message]) -> Result<String, String> {
        let endpoint = format!("{}/v1/chat/completions", self.base_url);
        let payload = json!({
            "model": std::env::var("OPENCHAT_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into()),
            "messages": messages.iter().map(|message| json!({
                "role": message.role,
                "content": message.content,
            })).collect::<Vec<Value>>(),
        });
        let response = ureq::AgentBuilder::new().timeout(Duration::from_secs(60)).build()
            .post(&endpoint)
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .send_json(payload)
            .map_err(|error| error.to_string())?;
        let body: Value = response.into_json().map_err(|error| error.to_string())?;
        body.pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| "provider response did not contain choices[0].message.content".into())
    }
}
