//! OpenCode attachment and management integration for openchat.

use serde::Deserialize;
use serde_json::{json, Value};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Deserialize)]
pub struct ServerInfo {
    pub version: String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub urls: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(rename = "projectID", default)]
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageContent {
    #[serde(rename = "type", default)]
    pub content_type: String,
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionMessage {
    pub id: String,
    #[serde(rename = "type", default)]
    pub msg_type: String,
    #[serde(default)]
    pub content: Option<Vec<MessageContent>>,
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    data: Option<T>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Clone)]
pub struct OpenCodeClient {
    base_url: String,
    password: Option<String>,
    agent: ureq::Agent,
}

impl OpenCodeClient {
    pub fn new(base_url: &str, password: Option<String>) -> Self {
        Self {
            base_url: base_url.trim().trim_end_matches('/').to_string(),
            password,
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(60))
                .build(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn auth(&self, request: ureq::Request) -> ureq::Request {
        if let Some(password) = &self.password {
            let credentials = format!("opencode:{}", password);
            let encoded =
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, credentials);
            request.set("Authorization", &format!("Basic {}", encoded))
        } else {
            request
        }
    }

    fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T, String> {
        let url = format!("{}/{}", self.base_url, path.trim_start_matches('/'));
        let request = self.auth(self.agent.get(&url));
        let response = request
            .call()
            .map_err(|error| format!("opencode GET {path} failed: {error}"))?;
        let body: ApiResponse<T> = response
            .into_json()
            .map_err(|error| format!("opencode GET {path} bad JSON: {error}"))?;
        body.data.ok_or_else(|| {
            body.description
                .unwrap_or_else(|| "missing data".to_string())
        })
    }

    fn post_json<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        payload: &Value,
    ) -> Result<T, String> {
        let url = format!("{}/{}", self.base_url, path.trim_start_matches('/'));
        let request = self.auth(self.agent.post(&url));
        let response = request
            .set("Content-Type", "application/json")
            .send_json(payload.clone())
            .map_err(|error| format!("opencode POST {path} failed: {error}"))?;
        let body: ApiResponse<T> = response
            .into_json()
            .map_err(|error| format!("opencode POST {path} bad JSON: {error}"))?;
        body.data.ok_or_else(|| {
            body.description
                .unwrap_or_else(|| "missing data".to_string())
        })
    }

    pub fn server_info(&self) -> Result<ServerInfo, String> {
        // /api/info returns ServerInfo directly (not wrapped in data).
        let url = format!("{}/api/info", self.base_url);
        let request = self.auth(self.agent.get(&url));
        let response = request
            .call()
            .map_err(|error| format!("opencode server info failed: {error}"))?;
        response
            .into_json()
            .map_err(|error| format!("opencode server info bad JSON: {error}"))
    }

    pub fn list_sessions(&self, limit: Option<usize>) -> Result<Vec<SessionInfo>, String> {
        let mut path = "/api/session".to_string();
        if let Some(limit) = limit {
            path = format!("{path}?limit={limit}");
        }
        self.get_json(&path)
    }

    pub fn create_session(&self, title: Option<&str>) -> Result<SessionInfo, String> {
        let mut payload = json!({});
        if let Some(title) = title {
            payload["title"] = json!(title);
        }
        self.post_json("/api/session", &payload)
    }

    pub fn send_prompt(&self, session_id: &str, text: &str) -> Result<String, String> {
        let path = format!("/api/session/{}/prompt", session_id);
        let payload = json!({ "text": text });
        let value: Value = self.post_json(&path, &payload)?;
        value
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "prompt response missing id".to_string())
    }

    pub fn messages(
        &self,
        session_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<SessionMessage>, String> {
        let mut path = format!("/api/session/{}/message", session_id);
        if let Some(limit) = limit {
            path = format!("{path}?limit={limit}&order=desc");
        }
        self.get_json(&path)
    }

    pub fn wait_for_idle(&self, session_id: &str, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let messages = self.messages(session_id, Some(20))?;
            if messages.iter().any(|message| message.msg_type == "idle") {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(500));
        }
        Err("timed out waiting for opencode session to become idle".to_string())
    }

    pub fn latest_assistant_text(&self, session_id: &str) -> Result<Option<String>, String> {
        let messages = self.messages(session_id, Some(50))?;
        for message in messages {
            if message.msg_type != "assistant" {
                continue;
            }
            if let Some(content) = &message.content {
                for part in content {
                    if part.content_type == "text" {
                        if let Some(text) = &part.text {
                            return Ok(Some(text.clone()));
                        }
                    }
                }
            }
            if let Some(text) = &message.text {
                return Ok(Some(text.clone()));
            }
        }
        Ok(None)
    }

    /// Create a session if needed, send a prompt, wait for completion, and return the assistant reply.
    pub fn ask(&self, session_id: Option<&str>, text: &str) -> Result<(String, String), String> {
        let session_id = match session_id {
            Some(id) => id.to_string(),
            None => self.create_session(Some("openchat"))?.id,
        };
        self.send_prompt(&session_id, text)?;
        self.wait_for_idle(&session_id, Duration::from_secs(120))?;
        let reply = self.latest_assistant_text(&session_id)?.unwrap_or_default();
        Ok((session_id, reply))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_info_deserializes() {
        let json = r#"{"id":"ses_1","title":"test","projectID":"p1"}"#;
        let info: SessionInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.id, "ses_1");
        assert_eq!(info.title.as_deref(), Some("test"));
    }

    #[test]
    fn message_deserializes() {
        let json = r#"{"id":"m1","type":"assistant","content":[{"type":"text","text":"hi"}]}"#;
        let message: SessionMessage = serde_json::from_str(json).unwrap();
        assert_eq!(message.msg_type, "assistant");
        assert_eq!(message.content.unwrap()[0].text.as_deref(), Some("hi"));
    }
}
