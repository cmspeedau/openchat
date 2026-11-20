//! Slop Law: prompt rules, linting, and logits-based antislop sampling.
//!
//! Sampling is only available for backends that expose generation logits
//! (e.g. llama.cpp or koboldcpp); see [`sampling::AntislopSampler`].
//! Inspired by https://github.com/miqdadbadjuber/anti-slop,
//! https://github.com/sam-paech/antislop-sampler, and
//! https://github.com/skew202/antislop.

pub mod lint;
pub mod prompt;
pub mod sampling;

pub use lint::{SlopLint, SlopLintConfig};
pub use sampling::{Logits, AntislopSampler};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lint_flags_known_slop() {
        let config = SlopLintConfig::default();
        let lint = SlopLint::new(config);
        let findings = lint.check("Her eyes sparkled like diamonds in the dark.");
        assert!(!findings.is_empty(), "cliché should be flagged");
    }

    #[test]
    fn lint_clean_text() {
        let lint = SlopLint::new(SlopLintConfig::default());
        assert!(lint.check("The librarian catalogued the shipment of braille hymnals.").is_empty());
    }

    #[test]
    fn sampler_suppresses_banned_token() {
        let logits = Logits::new(vec![("chasm".into(), 5.0), ("gulf".into(), 4.9), ("reef".into(), 0.1)]);
        let mut sampler = AntislopSampler::new(vec!["chasm".into()], 0.7);
        let chosen = sampler.sample(&logits, &mut |token| token == "chasm").unwrap();
        assert_ne!(chosen, "chasm");
    }

    #[test]
    fn sampler_all_excluded_is_error() {
        let logits = Logits::new(vec![("chasm".into(), 1.0)]);
        let mut sampler = AntislopSampler::new(vec!["chasm".into()], 0.7);
        assert!(sampler.sample(&logits, &mut |token| token == "chasm").is_err());
    }
}
