use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

pub const CONFIG_FILENAME: &str = ".hardware.toml";

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {source}")]
    Parse {
        path: String,
        source: toml::de::Error,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareConfig {
    pub schema: u32,
    pub kicad: KicadConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KicadConfig {
    pub project: Option<String>,
    pub schematic: String,
    pub pcb: Option<String>,
}

impl HardwareConfig {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.display().to_string(),
            source,
        })?;

        toml::from_str(&text).map_err(|source| ConfigError::Parse {
            path: path.display().to_string(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_repository_configuration() {
        let config: HardwareConfig = toml::from_str(
            r#"
schema = 1

[kicad]
project = "hardware/robot.kicad_pro"
schematic = "hardware/robot.kicad_sch"
pcb = "hardware/robot.kicad_pcb"
"#,
        )
        .expect("valid hardware configuration");

        assert_eq!(config.schema, 1);
        assert_eq!(config.kicad.schematic, "hardware/robot.kicad_sch");
    }
}
