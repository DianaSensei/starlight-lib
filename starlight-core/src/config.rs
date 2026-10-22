use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

const CONFIG_DIR_ENV: &str = "CONFIG_DIR";
const PROFILE_ENV: &str = "PROFILE";
const DEFAULT_CONFIG_DIR: &str = "config";
const DEFAULT_PROFILE: &str = "development";
const ENV_SEPARATOR: &str = "__";

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to load configuration: {0}")]
    Load(#[from] config::ConfigError),
    #[error("invalid configuration: {0}")]
    Invalid(String),
}

/// Validation run right after loading so a misconfigured service stops at startup.
pub trait Validate {
    fn validate(&self) -> Result<(), ConfigError>;
}

/// Active profile from `PROFILE` (default `development`), lowercased.
pub fn profile() -> String {
    std::env::var(PROFILE_ENV)
        .map(|value| value.to_lowercase())
        .unwrap_or_else(|_| DEFAULT_PROFILE.to_owned())
}

/// Loads `<CONFIG_DIR>/default.yml`, `<CONFIG_DIR>/<profile>.yml`, then `SECTION__KEY` env variables.
pub fn load<T: DeserializeOwned + Validate>() -> Result<T, ConfigError> {
    let dir = std::env::var(CONFIG_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_CONFIG_DIR));
    load_from(&dir, &profile())
}

pub fn load_from<T: DeserializeOwned + Validate>(
    dir: &Path,
    profile: &str,
) -> Result<T, ConfigError> {
    let file =
        |name: &str| config::File::with_name(&dir.join(name).to_string_lossy()).required(false);
    let loaded: T = config::Config::builder()
        .add_source(file("default"))
        .add_source(file(profile))
        .add_source(config::Environment::default().separator(ENV_SEPARATOR))
        .build()?
        .try_deserialize()?;
    loaded.validate()?;
    Ok(loaded)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Deserialize)]
    struct Sample {
        server: Server,
    }

    #[derive(Debug, Deserialize)]
    struct Server {
        port: u16,
        name: String,
    }

    impl Validate for Sample {
        fn validate(&self) -> Result<(), ConfigError> {
            if self.server.port == 0 {
                return Err(ConfigError::Invalid("server.port must not be 0".into()));
            }
            Ok(())
        }
    }

    fn write_config(dir: &Path, name: &str, body: &str) {
        fs::write(dir.join(format!("{name}.yml")), body).unwrap();
    }

    #[test]
    fn profile_file_overrides_default_file() {
        let dir = std::env::temp_dir().join(format!("starlight-config-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&dir).unwrap();
        write_config(&dir, "default", "server:\n  port: 8080\n  name: default\n");
        write_config(&dir, "staging", "server:\n  name: staging\n");

        let loaded: Sample = load_from(&dir, "staging").unwrap();

        assert_eq!(loaded.server.port, 8080);
        assert_eq!(loaded.server.name, "staging");
    }

    #[test]
    fn validation_failure_is_reported() {
        let dir = std::env::temp_dir().join(format!("starlight-config-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&dir).unwrap();
        write_config(&dir, "default", "server:\n  port: 0\n  name: x\n");

        let result = load_from::<Sample>(&dir, "development");

        assert!(matches!(result, Err(ConfigError::Invalid(_))));
    }
}
