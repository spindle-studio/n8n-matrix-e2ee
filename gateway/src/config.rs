use std::{env, path::PathBuf};

use thiserror::Error;

pub(crate) const DEFAULT_PORT: u16 = 8080;
pub(crate) const DEFAULT_DATA_DIR: &str = "/data";

#[derive(Debug, Clone)]
pub(crate) struct Config {
    pub(crate) homeserver_url: String,
    pub(crate) matrix_user: String,
    pub(crate) matrix_password: String,
    pub(crate) api_secret_token: String,
    pub(crate) port: u16,
    pub(crate) data_dir: PathBuf,
}

#[derive(Debug, Error)]
pub(crate) enum ConfigError {
    #[error("{0} must be set")]
    MissingEnvironment(&'static str),
    #[error("PORT must be a valid TCP port: {0}")]
    InvalidPort(#[from] std::num::ParseIntError),
}

impl Config {
    pub(crate) fn from_env() -> Result<Self, ConfigError> {
        let required = |name| {
            env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
                .ok_or(ConfigError::MissingEnvironment(name))
        };

        Ok(Self {
            homeserver_url: required("HOMESERVER_URL")?,
            matrix_user: required("MATRIX_USER")?,
            matrix_password: required("MATRIX_PASSWORD")?,
            api_secret_token: required("API_SECRET_TOKEN")?,
            port: env::var("PORT")
                .ok()
                .map(|value| value.parse())
                .transpose()?
                .unwrap_or(DEFAULT_PORT),
            data_dir: env::var("DATA_DIR")
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn configuration_uses_optional_defaults() {
        let _guard = env_lock().lock().expect("env lock should not be poisoned");
        unsafe {
            env::remove_var("API_SECRET_TOKEN");
            env::remove_var("PORT");
            env::remove_var("DATA_DIR");
            env::set_var("HOMESERVER_URL", "https://matrix.example.org");
            env::set_var("API_SECRET_TOKEN", "gateway-secret");
            env::set_var("MATRIX_USER", "@bot:example.org");
            env::set_var("MATRIX_PASSWORD", "secret");
        }

        let config = Config::from_env().expect("configuration should parse");

        assert_eq!(config.homeserver_url, "https://matrix.example.org");
        assert_eq!(config.api_secret_token, "gateway-secret");
        assert_eq!(config.port, DEFAULT_PORT);
        assert_eq!(config.data_dir, PathBuf::from(DEFAULT_DATA_DIR));
    }

    #[test]
    fn configuration_uses_custom_data_dir() {
        let _guard = env_lock().lock().expect("env lock should not be poisoned");
        unsafe {
            env::set_var("HOMESERVER_URL", "https://matrix.example.org");
            env::set_var("API_SECRET_TOKEN", "gateway-secret");
            env::set_var("MATRIX_USER", "@bot:example.org");
            env::set_var("MATRIX_PASSWORD", "secret");
            env::set_var("DATA_DIR", " ./data ");
        }

        let config = Config::from_env().expect("configuration should parse");
        assert_eq!(config.data_dir, PathBuf::from("./data"));

        unsafe {
            env::remove_var("DATA_DIR");
        }
    }

    #[test]
    fn configuration_requires_homeserver_url() {
        let _guard = env_lock().lock().expect("env lock should not be poisoned");
        unsafe {
            env::remove_var("HOMESERVER_URL");
            env::set_var("API_SECRET_TOKEN", "gateway-secret");
            env::set_var("MATRIX_USER", "@bot:example.org");
            env::set_var("MATRIX_PASSWORD", "secret");
        }

        assert!(matches!(
            Config::from_env(),
            Err(ConfigError::MissingEnvironment("HOMESERVER_URL"))
        ));
    }

    #[test]
    fn configuration_requires_api_secret_token() {
        let _guard = env_lock().lock().expect("env lock should not be poisoned");
        unsafe {
            env::remove_var("API_SECRET_TOKEN");
            env::set_var("HOMESERVER_URL", "https://matrix.example.org");
            env::set_var("MATRIX_USER", "@bot:example.org");
            env::set_var("MATRIX_PASSWORD", "secret");
        }

        assert!(matches!(
            Config::from_env(),
            Err(ConfigError::MissingEnvironment("API_SECRET_TOKEN"))
        ));
    }
}
