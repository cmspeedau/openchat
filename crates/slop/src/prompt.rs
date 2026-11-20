//! System-prompt rules that steer models away from slop.

/// Returns the Slop Law system-prompt block. Append to your system prompt.
pub fn slop_law_prompt() -> String {
    concat!(
        "Slop Law:\n",
        "1. Never use stock AI phrases (delve into, in the realm of, it's important to note, a testament to).\n",
        "2. No clichéd imagery (sparkling eyes, shivers down spines, time standing still).\n",
        "3. Prefer concrete, specific nouns over abstractions; one vivid detail beats three vague ones.\n",
        "4. Vary sentence length; do not begin successive sentences the same way.\n",
        "5. If a sentence could appear in any answer about any topic, delete it.\n"
    )
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::slop_law_prompt;

    #[test]
    fn prompt_mentions_rules() {
        let prompt = slop_law_prompt();
        assert!(prompt.contains("delve into"));
        assert!(prompt.starts_with("Slop Law:"));
    }
}
