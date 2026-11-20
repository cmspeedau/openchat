//! Slop linter: flags overused phrases, clichés, and stock AI patterns.

use serde::Deserialize;
use std::collections::BTreeSet;

/// Configuration for the linter; serializable so it can ship as TOML/JSON.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct SlopLintConfig {
    /// Exact phrases that are always flagged.
    pub phrases: Vec<String>,
    /// Clichés and stock imagery to flag.
    pub cliches: Vec<String>,
    /// Words that alone are not slop but heavy repetition reads as AI output.
    pub repeat_watch: Vec<String>,
    /// Flag a repeat_watch word if it appears more than this many times.
    pub repeat_limit: usize,
}

impl Default for SlopLintConfig {
    fn default() -> Self {
        Self {
            phrases: [
                "elicit a sense of",
                "it's important to note that",
                "delve into",
                "navigating the",
                "in the realm of",
                "a testament to",
            ]
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
            cliches: [
                "eyes sparkled like",
                "heart pounded in her chest",
                "shivers down her spine",
                "a chill ran down",
                "time stood still",
            ]
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
            repeat_watch: ["tapestry", "testament", "myriad", "nuanced", "multifaceted"]
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            repeat_limit: 2,
        }
    }
}

/// One lint finding, reported to the UI or CI.
#[derive(Clone, Debug, PartialEq)]
pub struct SlopFinding {
    pub kind: &'static str,
    pub text: String,
    pub offset: usize,
}

/// A linter built from a [`SlopLintConfig`].
#[derive(Clone, Debug)]
pub struct SlopLint {
    config: SlopLintConfig,
}

impl SlopLint {
    pub fn new(config: SlopLintConfig) -> Self {
        Self { config }
    }

    /// Check a completion; returns all findings in position order.
    pub fn check(&self, text: &str) -> Vec<SlopFinding> {
        let lower = text.to_lowercase();
        let mut findings = Vec::new();
        let mut reported = BTreeSet::new();
        for phrase in self.config.phrases.iter().chain(&self.config.cliches) {
            let needle = phrase.to_lowercase();
            let mut start = 0;
            while let Some(offset) = lower[start..].find(&needle) {
                let offset = start + offset;
                if reported.insert((needle.clone(), offset)) {
                    findings.push(SlopFinding {
                        kind: if self.config.phrases.contains(phrase) {
                            "phrase"
                        } else {
                            "cliche"
                        },
                        text: (*phrase).to_owned(),
                        offset,
                    });
                }
                start = offset + needle.len().max(1);
            }
        }
        for word in &self.config.repeat_watch {
            let needle = word.to_lowercase();
            let count = lower.matches(&needle).count();
            if count > self.config.repeat_limit {
                findings.push(SlopFinding {
                    kind: "repeat",
                    text: format!(
                        "{word} appears {count} times (limit {})",
                        self.config.repeat_limit
                    ),
                    offset: usize::MAX,
                });
            }
        }
        findings.sort_by_key(|finding| finding.offset);
        findings
    }
}
