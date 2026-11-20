//! A small process CPU governor for child workloads.
//!
//! CPU limiting is implemented by duty-cycling the launched process on Unix
//! (SIGSTOP/SIGCONT). The configured percentage is a single-process CPU budget;
//! it is not a system-wide governor or a hard real-time guarantee.

use std::{
    io,
    process::{Child, Command, ExitStatus},
    sync::{Arc, Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};
use sysinfo::{Pid, ProcessesToUpdate, System};

/// Configuration for the built-in child-process governor.
#[derive(Clone, Debug, PartialEq)]
pub struct GovernorConfig {
    /// Maximum CPU fraction for each governed child, from 1 to 100 percent.
    pub max_cpu_percent: u8,
    /// Unix nice value (-20 is highest priority, 19 is lowest). Defaults to 0.
    pub nice_level: i8,
    /// Maximum number of simultaneously launched governed children.
    pub thread_cap: usize,
    /// Length of one CPU duty-cycle period.
    pub sample_interval: Duration,
}

impl Default for GovernorConfig {
    fn default() -> Self {
        Self {
            max_cpu_percent: 100,
            nice_level: 0,
            thread_cap: 1,
            sample_interval: Duration::from_millis(100),
        }
    }
}

impl GovernorConfig {
    /// Validate configured limits before creating a governor.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if !(1..=100).contains(&self.max_cpu_percent) {
            return Err(ConfigError("max_cpu_percent must be between 1 and 100"));
        }
        if !(-20..=19).contains(&self.nice_level) {
            return Err(ConfigError("nice_level must be between -20 and 19"));
        }
        if self.thread_cap == 0 {
            return Err(ConfigError("thread_cap must be greater than zero"));
        }
        if self.sample_interval.is_zero() {
            return Err(ConfigError("sample_interval must be greater than zero"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfigError(&'static str);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ConfigError {}

/// Samples CPU use for a process using sysinfo's per-process measurements.
pub struct ProcessSampler {
    system: System,
}

impl Default for ProcessSampler {
    fn default() -> Self {
        Self {
            system: System::new(),
        }
    }
}

impl ProcessSampler {
    /// Create a sampler with sysinfo's default process refresh configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Refresh and return the process's CPU percentage, or `None` if it is gone.
    pub fn cpu_percent(&mut self, pid: u32) -> Option<f32> {
        let pid = Pid::from_u32(pid);
        self.system
            .refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        self.system.process(pid).map(|process| process.cpu_usage())
    }
}

#[derive(Default)]
struct Slots {
    active: Mutex<usize>,
    available: Condvar,
}

impl Slots {
    fn acquire(self: &Arc<Self>, cap: usize) -> Slot {
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        while *active >= cap {
            active = self
                .available
                .wait(active)
                .unwrap_or_else(|e| e.into_inner());
        }
        *active += 1;
        Slot(Arc::clone(self))
    }
}

struct Slot(Arc<Slots>);

impl Drop for Slot {
    fn drop(&mut self) {
        let mut active = self.0.active.lock().unwrap_or_else(|e| e.into_inner());
        *active -= 1;
        self.0.available.notify_one();
    }
}

/// A child launched through [`Governor::launch`], holding a concurrency slot.
pub struct ManagedChild {
    child: Child,
    _slot: Slot,
}

impl ManagedChild {
    pub fn id(&self) -> u32 {
        self.child.id()
    }
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child.wait()
    }
    pub fn kill(&mut self) -> io::Result<()> {
        self.child.kill()
    }
}

/// Configured launcher, process sampler and duty-cycle controller.
pub struct Governor {
    config: GovernorConfig,
    slots: Arc<Slots>,
}

impl Governor {
    pub fn new(config: GovernorConfig) -> Result<Self, ConfigError> {
        config.validate()?;
        Ok(Self {
            config,
            slots: Arc::default(),
        })
    }

    pub fn config(&self) -> &GovernorConfig {
        &self.config
    }

    /// Spawn a child after applying the configured nice level and thread cap.
    /// A slot is held until the returned handle is dropped.
    pub fn launch(&self, command: &mut Command) -> io::Result<ManagedChild> {
        let slot = self.slots.acquire(self.config.thread_cap);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            let nice = self.config.nice_level;
            // SAFETY: the pre-exec closure only invokes async-signal-safe setpriority.
            unsafe {
                command.pre_exec(move || {
                    if libc::setpriority(libc::PRIO_PROCESS, 0, i32::from(nice)) == -1 {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        #[cfg(not(unix))]
        if self.config.nice_level != 0 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "nice priority is unsupported on this platform",
            ));
        }
        match command.spawn() {
            Ok(child) => Ok(ManagedChild { child, _slot: slot }),
            Err(error) => Err(error),
        }
    }

    /// Duty-cycle a child until it exits, allowing at most the configured
    /// fraction of each period to run. On Unix the child is paused between slices.
    pub fn run_throttled(&self, child: &mut ManagedChild) -> io::Result<ExitStatus> {
        #[cfg(not(unix))]
        {
            let _ = child;
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "process duty-cycle throttling is unsupported on this platform",
            ));
        }
        #[cfg(unix)]
        {
            if self.config.max_cpu_percent == 100 {
                return child.wait();
            }
            let period = self.config.sample_interval;
            let running = period.mul_f64(f64::from(self.config.max_cpu_percent) / 100.0);
            let paused = period.saturating_sub(running);
            loop {
                let start = Instant::now();
                thread::sleep(running);
                if let Some(status) = child.try_wait()? {
                    return Ok(status);
                }
                signal(child.id(), libc::SIGSTOP)?;
                thread::sleep(paused);
                if let Some(status) = child.try_wait()? {
                    return Ok(status);
                }
                signal(child.id(), libc::SIGCONT)?;
                let elapsed = start.elapsed();
                if elapsed < period {
                    thread::sleep(period - elapsed);
                }
            }
        }
    }
}

#[cfg(unix)]
fn signal(pid: u32, signal: i32) -> io::Result<()> {
    // SAFETY: kill is called with the live child PID and a valid signal constant.
    if unsafe { libc::kill(pid as libc::pid_t, signal) } == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        assert!(Governor::new(GovernorConfig::default()).is_ok());
    }

    #[test]
    fn config_rejects_invalid_cpu_percent_and_thread_cap() {
        let mut config = GovernorConfig {
            max_cpu_percent: 0,
            ..GovernorConfig::default()
        };
        assert!(config.validate().is_err());
        config.max_cpu_percent = 101;
        assert!(config.validate().is_err());
        config.max_cpu_percent = 50;
        config.thread_cap = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn config_rejects_invalid_nice_level_and_period() {
        let mut config = GovernorConfig {
            nice_level: 20,
            ..GovernorConfig::default()
        };
        assert!(config.validate().is_err());
        config.nice_level = 0;
        config.sample_interval = Duration::ZERO;
        assert!(config.validate().is_err());
    }

    #[test]
    fn child_launch_and_wait_work() {
        let governor = Governor::new(GovernorConfig::default()).unwrap();
        #[cfg(unix)]
        let mut command = {
            let mut c = Command::new("true");
            c
        };
        #[cfg(windows)]
        let mut command = {
            let mut c = Command::new("cmd");
            c.args(["/C", "exit 0"]);
            c
        };
        let status = governor.launch(&mut command).unwrap().wait().unwrap();
        assert!(status.success());
    }
}
