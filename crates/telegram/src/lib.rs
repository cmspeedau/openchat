//! Telegram bot integration for openchat.
//!
//! The bot long-polls the Telegram Bot API, forwards user messages to the
//! configured OpenAI-compatible provider (or a mock when none is configured),
//! and replies with the generated answer.

use serde::Deserialize;
use serde_json::{json, Value};
use std::env;
use std::thread;
use std::time::Duration;
use tracing::{error, info};

/// Telegram user.
#[derive(Debug, Clone, Deserialize)]
pub struct TelegramUser {
    pub id: i64,
    pub username: Option<String>,
    pub first_name: Option<String>,
}

/// Telegram chat.
#[derive(Debug, Clone, Deserialize)]
pub struct TelegramChat {
    pub id: i64,
    #[serde(rename = "type")]
    pub chat_type: Option<String>,
}

/// A Telegram message.
#[derive(Debug, Clone, Deserialize)]
pub struct TelegramMessage {
    pub message_id: i64,
    pub chat: TelegramChat,
    pub from: Option<TelegramUser>,
    pub text: Option<String>,
}

/// A Telegram update.
#[derive(Debug, Clone, Deserialize)]
pub struct Update {
    pub update_id: i64,
    pub message: Option<TelegramMessage>,
}

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
}

/// Minimal Telegram Bot API client built on ureq.
#[derive(Clone)]
pub struct TelegramClient {
    token: String,
    agent: ureq::Agent,
}

impl TelegramClient {
    pub fn new(token: String) -> Self {
        Self {
            token,
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(60))
                .build(),
        }
    }

    pub fn get_updates(
        &self,
        offset: Option<i64>,
        timeout_secs: u64,
    ) -> Result<Vec<Update>, String> {
        let url = format!("https://api.telegram.org/bot{}/getUpdates", self.token);
        let mut request = self.agent.get(&url);
        if let Some(offset) = offset {
            request = request.query("offset", &offset.to_string());
        }
        request = request.query("timeout", &timeout_secs.to_string());
        let response: ApiResponse<Vec<Update>> = request
            .call()
            .map_err(|error| format!("telegram getUpdates failed: {error}"))?
            .into_json()
            .map_err(|error| format!("telegram getUpdates bad JSON: {error}"))?;
        if !response.ok {
            return Err(response
                .description
                .unwrap_or_else(|| "telegram getUpdates returned ok=false".to_string()));
        }
        Ok(response.result.unwrap_or_default())
    }

    pub fn send_message(&self, chat_id: i64, text: &str) -> Result<(), String> {
        for chunk in split_message(text, 4000) {
            let url = format!("https://api.telegram.org/bot{}/sendMessage", self.token);
            let body = json!({
                "chat_id": chat_id,
                "text": chunk,
            });
            let response: ApiResponse<Value> = self
                .agent
                .post(&url)
                .set("Content-Type", "application/json")
                .send_json(body)
                .map_err(|error| format!("telegram sendMessage failed: {error}"))?
                .into_json()
                .map_err(|error| format!("telegram sendMessage bad JSON: {error}"))?;
            if !response.ok {
                return Err(response
                    .description
                    .unwrap_or_else(|| "telegram sendMessage returned ok=false".to_string()));
            }
        }
        Ok(())
    }
}

/// Split a long message into chunks that fit Telegram's 4096 character limit.
pub fn split_message(text: &str, limit: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    if text.chars().count() <= limit {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if current.chars().count() >= limit {
            chunks.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Welcome text for /start.
pub fn welcome_text() -> &'static str {
    "Hi! I'm an openchat bot. Send me a message and I'll reply using the configured model.\n\
     Commands: /start, /help"
}

/// Help text.
pub fn help_text() -> &'static str {
    "Send any text message to chat. The bot uses OPENCHAT_BASE_URL and OPENCHAT_API_KEY when set, \
     otherwise it replies with a mock echo."
}

/// Generate a reply for a user message.
pub fn complete(user_text: &str, slop_law: bool) -> Result<String, String> {
    let api_key = env::var("OPENCHAT_API_KEY").ok();
    let base_url = env::var("OPENCHAT_BASE_URL").ok();
    let model = env::var("OPENCHAT_MODEL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "gpt-4o-mini".to_string());

    if let (Some(api_key), Some(base_url)) = (api_key, base_url) {
        if !api_key.trim().is_empty() && !base_url.trim().is_empty() {
            let mut messages: Vec<Value> = Vec::new();
            if slop_law {
                messages.push(json!({
                    "role": "system",
                    "content": openchat_slop::prompt::slop_law_prompt(),
                }));
            }
            messages.push(json!({
                "role": "user",
                "content": user_text,
            }));
            let endpoint = format!(
                "{}/v1/chat/completions",
                base_url.trim().trim_end_matches('/')
            );
            let payload = json!({
                "model": model,
                "messages": messages,
                "stream": false,
            });
            let agent = ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(60))
                .build();
            let response: Value = agent
                .post(&endpoint)
                .set("Authorization", &format!("Bearer {}", api_key))
                .set("Content-Type", "application/json")
                .send_json(payload)
                .map_err(|error| format!("provider request failed: {error}"))?
                .into_json()
                .map_err(|error| format!("provider bad JSON: {error}"))?;
            if let Some(error) = response.get("error") {
                return Err(format!("provider error: {error}"));
            }
            if let Some(content) = response
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
            {
                return Ok(content.to_string());
            }
            return Err("provider response missing choices/0/message/content".to_string());
        }
    }

    Ok(format!("Mock reply: {user_text}"))
}

/// Process a single update: reply to text messages and commands.
pub fn handle_update(client: &TelegramClient, update: &Update, slop_law: bool) {
    let Some(message) = &update.message else {
        return;
    };
    let Some(text) = &message.text else {
        return;
    };
    let chat_id = message.chat.id;
    let reply = match text.trim() {
        "/start" => welcome_text().to_string(),
        "/help" => help_text().to_string(),
        other => match complete(other, slop_law) {
            Ok(reply) => reply,
            Err(error) => format!("Sorry, I couldn't generate a reply: {error}"),
        },
    };
    if let Err(error) = client.send_message(chat_id, &reply) {
        error!("failed to send telegram reply: {error}");
    }
}

/// Run the bot until interrupted.
pub fn run(token: String, slop_law: bool) -> Result<(), String> {
    let client = TelegramClient::new(token);
    let mut offset: Option<i64> = None;
    info!("telegram bot started");
    loop {
        match client.get_updates(offset, 30) {
            Ok(updates) => {
                for update in updates {
                    offset = Some(update.update_id + 1);
                    let client = client.clone();
                    thread::spawn(move || handle_update(&client, &update, slop_law));
                }
            }
            Err(error) => {
                error!("telegram polling failed: {error}");
                thread::sleep(Duration::from_secs(5));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_short_message() {
        assert_eq!(split_message("hello", 10), vec!["hello".to_string()]);
    }

    #[test]
    fn split_long_message() {
        let text = "a".repeat(10);
        let chunks = split_message(&text, 4);
        assert_eq!(chunks, vec!["aaaa", "aaaa", "aa"]);
    }

    #[test]
    fn split_empty() {
        assert_eq!(split_message("", 4), vec![String::new()]);
    }

    #[test]
    fn parse_update_json() {
        let json = r#"{"update_id":123,"message":{"message_id":1,"chat":{"id":42,"type":"private"},"text":"hi"}}"#;
        let update: Update = serde_json::from_str(json).unwrap();
        assert_eq!(update.update_id, 123);
        assert_eq!(update.message.unwrap().text.unwrap(), "hi");
    }

    #[test]
    fn complete_mock_without_env() {
        std::env::remove_var("OPENCHAT_API_KEY");
        std::env::remove_var("OPENCHAT_BASE_URL");
        let reply = complete("hello", false).unwrap();
        assert_eq!(reply, "Mock reply: hello");
    }

    #[test]
    fn welcome_and_help() {
        assert!(welcome_text().contains("openchat bot"));
        assert!(help_text().contains("OPENCHAT_BASE_URL"));
    }
}
