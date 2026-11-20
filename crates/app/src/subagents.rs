use std::sync::mpsc::Sender;

/// Real subagent orchestration: background workers run bounded tasks
/// and report progress through channels consumed by the UI thread.
/// The previous demo-only status ticker is replaced by tasks that do
/// actual work (currently: Slop Law lint passes over text).

pub enum TaskKind {
    /// Run the Slop Law linter over the given text.
    LintSlop(String),
}

#[derive(Clone, Debug)]
pub enum TaskEvent {
    Started { task_id: u64, name: String },
    Finished { task_id: u64, name: String, summary: String },
    Failed { task_id: u64, name: String, error: String },
}

/// A subagent runs one task on a background thread and streams
/// TaskEvents to the provided sender.
pub struct Subagent {
    pub name: String,
}

impl Subagent {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string() }
    }

    /// Spawn the task on its own thread. Events arrive on `events`.
    pub fn spawn(&self, task_id: u64, task: TaskKind, events: Sender<TaskEvent>) {
        let name = self.name.clone();
        std::thread::spawn(move || {
            let _ = events.send(TaskEvent::Started { task_id, name: name.clone() });
            match task {
                TaskKind::LintSlop(text) => match run_lint(&text) {
                    Ok(summary) => {
                        let _ = events.send(TaskEvent::Finished { task_id, name, summary });
                    }
                    Err(error) => {
                        let _ = events.send(TaskEvent::Failed { task_id, name, error });
                    }
                },
            }
        });
    }
}

fn run_lint(text: &str) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err("nothing to lint: text is empty".into());
    }
    let lint = openchat_slop::SlopLint::new(openchat_slop::SlopLintConfig::default());
    let findings = lint.check(text);
    if findings.is_empty() {
        Ok("lint passed: no slop found".into())
    } else {
        Ok(format!("lint found {} slop finding(s)", findings.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn lint_task_reports_findings() {
        let (tx, rx) = mpsc::channel();
        Subagent::new("Linter").spawn(1, TaskKind::LintSlop("a testament to her journey".into()), tx);
        let mut saw_finished = false;
        for event in rx.iter() {
            match event {
                TaskEvent::Finished { summary, .. } => {
                    assert!(summary.contains("1 slop finding"));
                    saw_finished = true;
                    break;
                }
                TaskEvent::Failed { error, .. } => panic!("unexpected failure: {error}"),
                TaskEvent::Started { .. } => {}
            }
        }
        assert!(saw_finished);
    }

    #[test]
    fn lint_task_fails_on_empty_text() {
        let (tx, rx) = mpsc::channel();
        Subagent::new("Linter").spawn(2, TaskKind::LintSlop("  ".into()), tx);
        let mut saw_failure = false;
        for event in rx.iter() {
            match event {
                TaskEvent::Failed { .. } => {
                    saw_failure = true;
                    break;
                }
                TaskEvent::Finished { .. } => panic!("empty text should not pass"),
                TaskEvent::Started { .. } => {}
            }
        }
        assert!(saw_failure);
    }
}
