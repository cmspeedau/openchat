//! Logits-based antislop sampling.
//!
//! Only meaningful for backends that expose per-token generation logits,
//! such as llama.cpp (via its server /v1/ completions with logit bias access)
//! or koboldcpp. Cloud APIs that do not expose logits cannot use this.

/// One candidate token with its logit.
#[derive(Clone, Debug, PartialEq)]
pub struct Logit {
    pub token: String,
    pub value: f32,
}

/// A full candidate distribution for one step, sorted by value.
#[derive(Clone, Debug, Default)]
pub struct Logits {
    pub entries: Vec<Logit>,
}

impl Logits {
    pub fn new(entries: Vec<(String, f32)>) -> Self {
        let mut entries: Vec<Logit> = entries
            .into_iter()
            .map(|(token, value)| Logit { token, value })
            .collect();
        entries.sort_by(|a, b| b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal));
        Self { entries }
    }
}

/// Antislop sampler: bans slop tokens (and their continuations) and samples
/// from what remains, temperature-scaled. Deterministic when `temperature <= 0`.
pub struct AntislopSampler {
    banned: Vec<String>,
    temperature: f32,
}

impl AntislopSampler {
    /// `banned` tokens are excluded at every step; `temperature` <= 0 makes
    /// sampling deterministic (argmax of the allowed distribution).
    pub fn new(banned: Vec<String>, temperature: f32) -> Self {
        Self { banned, temperature }
    }

    /// Sample one token. `is_slop` receives each candidate token and reports
    /// whether it is a slop continuation (so callers can ban "chasm"-like
    /// openers mid-stream, matching antislop-sampler's suffix matching).
    pub fn sample(
        &mut self,
        logits: &Logits,
        is_slop: &mut dyn FnMut(&str) -> bool,
    ) -> Result<String, String> {
        let allowed: Vec<&Logit> = logits
            .entries
            .iter()
            .filter(|entry| !self.banned.contains(&entry.token) && !is_slop(&entry.token))
            .collect();
        let Some(first) = allowed.first() else {
            return Err("every candidate token is banned; relax the slop list".into());
        };
        if self.temperature <= 0.0 || allowed.len() == 1 {
            return Ok(first.token.clone());
        }
        let max = first.value;
        let weights: Vec<f32> = allowed
            .iter()
            .map(|entry| ((entry.value - max) / self.temperature).exp())
            .collect();
        let total: f32 = weights.iter().sum();
        let mut pick = fastrand_pick();
        for (entry, weight) in allowed.iter().zip(&weights) {
            pick -= weight / total;
            if pick <= 0.0 {
                return Ok(entry.token.clone());
            }
        }
        Ok(allowed[allowed.len() - 1].token.clone())
    }
}

// Deterministic-enough xorshift so the crate has no dependency on rand.
fn fastrand_pick() -> f32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
        .unwrap_or(0x9E37_79B9_7F4A_7C15);
    let mut x = nanos | 1;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    (x % 1_000_000) as f32 / 1_000_000.0
}
