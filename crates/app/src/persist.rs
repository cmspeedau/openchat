//! JSON persistence for conversations and messages.
//!
//! Stored manually with serde_json::json!/Value so the app crate does not
//! need serde derive on its UI types.

use crate::{Conversation, Message, ProviderChoice, Role};
use serde_json::{json, Value};

pub const DEFAULT_PATH: &str = "openchat-state.json";

pub fn save(path: &str, conversations: &[Conversation], messages: &[Message], selected: Option<u64>) {
    let state = json!({
        "conversations": conversations.iter().map(|c| json!({
            "id": c.id,
            "title": c.title,
            "preview": c.preview,
            "provider": provider_to_str(c.provider),
            "model_override": c.model_override,
            "base_url_override": c.base_url_override,
        })).collect::<Vec<Value>>(),
        "messages": messages.iter().map(|m| json!({
            "role": role_to_str(m.role),
            "body": m.body,
        })).collect::<Vec<Value>>(),
        "selected": selected,
    });
    match serde_json::to_string_pretty(&state) {
        Ok(text) => {
            if let Err(error) = std::fs::write(path, text) {
                eprintln!("failed to save state: {error}");
            }
        }
        Err(error) => eprintln!("failed to save state: {error}"),
    }
}

pub fn load(path: &str) -> Option<(Vec<Conversation>, Vec<Message>, Option<u64>)> {
    let text = std::fs::read_to_string(path).ok()?;
    let state: Value = serde_json::from_str(&text).ok()?;
    let conversations = state.get("conversations")?.as_array()?.iter().filter_map(|c| {
        Some(Conversation {
            id: c.get("id")?.as_u64()?,
            title: c.get("title")?.as_str()?.to_string(),
            preview: c.get("preview")?.as_str()?.to_string(),
            provider: provider_from_str(c.get("provider")?.as_str()?),
            model_override: c.get("model_override")?.as_str().unwrap_or_default().to_string(),
            base_url_override: c.get("base_url_override")?.as_str().unwrap_or_default().to_string(),
        })
    }).collect::<Vec<_>>();
    if conversations.is_empty() { return None; }
    let messages = state.get("messages").and_then(Value::as_array).map(|msgs| {
        msgs.iter().filter_map(|m| {
            Some(Message {
                role: role_from_str(m.get("role").and_then(Value::as_str)?),
                body: m.get("body")?.as_str()?.to_string(),
            })
        }).collect::<Vec<_>>()
    }).unwrap_or_default();
    Some((conversations, messages, state.get("selected").and_then(Value::as_u64)))
}

fn provider_to_str(choice: ProviderChoice) -> &'static str {
    match choice { ProviderChoice::Mock => "mock", ProviderChoice::OpenAICompat => "openai" }
}
fn provider_from_str(value: &str) -> ProviderChoice {
    match value { "openai" => ProviderChoice::OpenAICompat, _ => ProviderChoice::Mock }
}
fn role_to_str(role: Role) -> &'static str {
    match role { Role::User => "user", Role::Assistant => "assistant", Role::System => "system" }
}
fn role_from_str(value: &str) -> Role {
    match value { "assistant" => Role::Assistant, "system" => Role::System, _ => Role::User }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_state() {
        let conversations = vec![Conversation { id: 1, title: "t".into(), preview: "p".into(),
            provider: ProviderChoice::OpenAICompat, model_override: "m".into(), base_url_override: String::new() }];
        let messages = vec![Message { role: Role::User, body: "hi".into() }];
        let dir = std::env::temp_dir().join("openchat-persist-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");
        save(path.to_str().unwrap(), &conversations, &messages, Some(1));
        let (loaded_convs, loaded_msgs, selected) = load(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded_convs.len(), 1);
        assert_eq!(loaded_convs[0].provider, ProviderChoice::OpenAICompat);
        assert_eq!(loaded_msgs, messages);
        assert_eq!(selected, Some(1));
        let _ = std::fs::remove_file(&path);
    }
}
