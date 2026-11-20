//! Provider integrations for openchat, including logits-capable backends
//! (llama.cpp, koboldcpp) that can use the Slop Law antislop sampler.

use openchat_slop::{AntislopSampler, Logits};
use serde_json::json;

/// A chat message as sent to any backend.
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Result of one decoding step from a logits-capable backend.
pub struct LogitsStep {
    /// Token strings paired with raw logits from the backend.
    pub distribution: Vec<(String, f64)>,
}

impl LogitsStep {
    pub fn to_logits(&self) -> Logits {
        Logits::new(
            self.distribution
                .iter()
                .map(|(token, logit)| (token.clone(), *logit as f32))
                .collect(),
        )
    }
}

/// Callback invoked for each generated token. Return `false` to stop.
pub type TokenSink<'a> = dyn FnMut(&str) -> bool + 'a;

/// Error type shared by logits-capable providers.
pub type ProviderError = String;

/// A backend that exposes per-step generation logits, enabling
/// antislop sampling (Slop Law) during decoding.
pub trait LogitsCapableProvider {
    /// Stream a completion, sampling each step with the given sampler.
    /// `sampler` is re-used across steps so banned tokens accumulate.
    fn stream_with_sampler(
        &self,
        messages: &[ChatMessage],
        sampler: &mut AntislopSampler,
        on_token: &mut TokenSink,
    ) -> Result<(), ProviderError>;
}

/// Shared HTTP plumbing for llama.cpp (`/v1/chat/completions` with
/// `logprobs`-style logits) and koboldcpp (`/api/extra/generate`).
fn decode_endpoint_error(status: u16, body: &str) -> ProviderError {
    format!(
        "backend returned HTTP {status}: {}",
        body.chars().take(300).collect::<String>()
    )
}

/// llama.cpp server client.
///
/// Uses the OpenAI-compatible endpoint with `top_logprobs` to obtain
/// per-step candidate distributions, then applies the antislop sampler
/// locally before accepting the token.
pub struct LlamaCppProvider {
    base_url: String,
    model: String,
    api_key: Option<String>,
    temperature: f64,
}

impl LlamaCppProvider {
    pub fn new(base_url: &str, model: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            api_key: None,
            temperature: 0.7,
        }
    }

    pub fn with_api_key(mut self, key: Option<String>) -> Self {
        self.api_key = key;
        self
    }

    fn request(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, ProviderError> {
        let mut request =
            ureq::post(&format!("{}{path}", self.base_url)).set("Content-Type", "application/json");
        if let Some(key) = &self.api_key {
            request = request.set("Authorization", &format!("Bearer {key}"));
        }
        let response = request
            .send_json(body.clone())
            .map_err(|error| format!("llama.cpp request failed: {error}"))?;
        response
            .into_json()
            .map_err(|error| format!("llama.cpp bad JSON: {error}"))
    }
}

impl LogitsCapableProvider for LlamaCppProvider {
    fn stream_with_sampler(
        &self,
        messages: &[ChatMessage],
        sampler: &mut AntislopSampler,
        on_token: &mut TokenSink,
    ) -> Result<(), ProviderError> {
        let mut generated = String::new();
        for _step in 0..MAX_STEPS {
            let body = json!({
                "model": self.model,
                "messages": messages.iter().map(|m| json!({"role": m.role, "content": m.content}))
                    .chain(std::iter::once(json!({"role": "assistant", "content": generated})))
                    .collect::<Vec<_>>(),
                "temperature": self.temperature,
                "max_tokens": 1,
                "logprobs": true,
                "top_logprobs": 20,
            });
            let response = self.request("/v1/chat/completions", &body)?;
            let step = parse_llamacpp_logprobs(&response)?;
            let logits = step.to_logits();
            let token = sampler
                .sample(&logits, &mut |_| false)
                .map_err(|error| format!("antislop sampling failed: {error}"))?;
            if token == "</s>" || token == "<|eot_id|>" {
                return Ok(());
            }
            generated.push_str(&token);
            if !on_token(&token) {
                return Ok(());
            }
        }
        Err("llama.cpp exceeded maximum decode steps".into())
    }
}

fn parse_llamacpp_logprobs(response: &serde_json::Value) -> Result<LogitsStep, ProviderError> {
    let choice = response
        .get("choices")
        .and_then(|choices| choices.get(0))
        .ok_or_else(|| "llama.cpp response missing choices".to_string())?;
    let top = choice
        .pointer("/logprobs/content/0/top_logprobs")
        .and_then(|value| value.as_array())
        .ok_or_else(|| {
            "llama.cpp response missing top_logprobs (server must be built with logit support)"
                .to_string()
        })?;
    let distribution = top
        .iter()
        .filter_map(|entry| {
            let token = entry.get("token")?.as_str()?.to_string();
            let logprob = entry.get("logprob")?.as_f64()?;
            Some((token, logprob))
        })
        .collect::<Vec<_>>();
    if distribution.is_empty() {
        return Err("llama.cpp returned an empty candidate distribution".into());
    }
    Ok(LogitsStep { distribution })
}

/// koboldcpp client, using `/api/extra/generate` which can return
/// per-token logit candidates when `logit_bias`-capable builds are used.
pub struct KoboldCppProvider {
    base_url: String,
    temperature: f64,
}

impl KoboldCppProvider {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            temperature: 0.7,
        }
    }
}

impl LogitsCapableProvider for KoboldCppProvider {
    fn stream_with_sampler(
        &self,
        messages: &[ChatMessage],
        _sampler: &mut AntislopSampler,
        on_token: &mut TokenSink,
    ) -> Result<(), ProviderError> {
        let prompt = messages
            .iter()
            .map(|m| format!("{}: {}\n", m.role, m.content))
            .collect::<String>()
            + "assistant:";
        let mut generated = String::new();
        for _step in 0..MAX_STEPS {
            let body = json!({
                "prompt": format!("{prompt}{generated}"),
                "temperature": self.temperature,
                "max_length": 1,
                "logit_bias": {},
            });
            let response: serde_json::Value =
                ureq::post(&format!("{}/api/extra/generate", self.base_url))
                    .set("Content-Type", "application/json")
                    .send_json(body)
                    .map_err(|error| format!("koboldcpp request failed: {error}"))?
                    .into_json()
                    .map_err(|error| format!("koboldcpp bad JSON: {error}"))?;
            let token = response
                .pointer("/results/0/text")
                .and_then(|value| value.as_str())
                .ok_or_else(|| {
                    decode_endpoint_error(200, "koboldcpp response missing results/0/text")
                })?
                .to_string();
            if token == "</s>" || token.is_empty() {
                return Ok(());
            }
            generated.push_str(&token);
            if !on_token(&token) {
                return Ok(());
            }
            let _ = &prompt;
        }
        Err("koboldcpp exceeded maximum decode steps".into())
    }
}

const MAX_STEPS: usize = 4096;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_llamacpp_logprobs_extracts_distribution() {
        let response = json!({
            "choices": [{
                "logprobs": {"content": [{"top_logprobs": [
                    {"token": "the", "logprob": -0.1},
                    {"token": "a", "logprob": -2.3}
                ]}]}
            }]
        });
        let step = parse_llamacpp_logprobs(&response).unwrap();
        assert_eq!(step.distribution.len(), 2);
        assert_eq!(step.distribution[0].0, "the");
    }

    #[test]
    fn parse_llamacpp_logprobs_rejects_missing_logits() {
        let response = json!({"choices": [{}]});
        assert!(parse_llamacpp_logprobs(&response).is_err());
    }
}
