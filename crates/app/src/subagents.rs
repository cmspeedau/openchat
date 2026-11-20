use std::time::{Duration, Instant};

/// Demonstration-only status: OpenChat does not currently run subagents.
#[derive(Clone)]
pub struct SubagentStatus {
    pub name: &'static str,
    steps: &'static [&'static str],
    step: usize,
    updated_at: Instant,
}
impl SubagentStatus {
    fn demo(name: &'static str, steps: &'static [&'static str]) -> Self {
        Self { name, steps, step: 0, updated_at: Instant::now() }
    }
    pub fn advance_demo(&mut self, now: Instant) {
        if now.duration_since(self.updated_at) >= Duration::from_secs(4) {
            self.step = (self.step + 1) % self.steps.len();
            self.updated_at = now;
        }
    }
    pub fn current_step(&self) -> &'static str { self.steps[self.step] }
}
pub fn demo_agents() -> Vec<SubagentStatus> {
    vec![
        SubagentStatus::demo("Researcher (demo)", &["Reviewing context", "Collecting notes", "Ready to report"]),
        SubagentStatus::demo("Reviewer (demo)", &["Checking consistency", "Finding edge cases", "Review complete"]),
    ]
}
