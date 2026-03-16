use serde::Deserialize;
use std::fs;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub debug: DebugConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DebugConfig {
    pub enable_profiler: bool,
}

impl Config {
    pub fn load() -> Self {
        let config_str = fs::read_to_string("config.toml")
            .expect("Failed to read config.toml! Ensure the file exists in the project root.");
        toml::from_str(&config_str).expect("Failed to parse config.toml!")
    }
}