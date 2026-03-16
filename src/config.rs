use serde::Deserialize;
use std::fs;

#[derive(Deserialize, Clone)]
pub struct DebugConfig {
    pub enable_profiler: bool,
}

impl DebugConfig {
    pub fn load() -> Self {
        let path = "config.toml";
        let config_str =
            fs::read_to_string(path).unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e));

        toml::from_str(&config_str)
            .unwrap_or_else(|e| panic!("Failed to parse {}: {}", path, e))
    }
}