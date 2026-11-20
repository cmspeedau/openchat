use serde::Deserialize;
use std::{fs, io, path::Path};
use tracing_subscriber::EnvFilter;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    pub model: Option<String>,
    pub endpoint: Option<String>,
}

#[derive(Debug)]
pub enum ConfigError {
    Io(io::Error),
    Parse(toml::de::Error),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "unable to read configuration: {error}"),
            Self::Parse(error) => write!(formatter, "unable to parse configuration: {error}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Load configuration from a TOML file.
pub fn load_config(path: impl AsRef<Path>) -> Result<Config, ConfigError> {
    let contents = fs::read_to_string(path).map_err(ConfigError::Io)?;
    toml::from_str(&contents).map_err(ConfigError::Parse)
}

/// Initialize formatted tracing output, respecting `RUST_LOG` when it is set.
pub fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}
