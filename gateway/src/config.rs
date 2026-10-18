use std::{collections::HashSet, env, path::PathBuf};

use thiserror::Error;

pub(crate) const DEFAULT_PORT: u16 = 8080;
pub(crate) const DEFAULT_DATA_DIR: &str = "/data";

#[derive(Debug, Clone)]
pub(crate) struct Config {
    pub(crate) homeserver_url: String,
    pub(crate) matrix_user: String,
    pub(crate) matrix_password: String,
    pub(crate) api_secret_token: String,
    pub(crate) allowed_inviters: HashSet<String>,
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
            allowed_inviters: parse_allowed_inviters(
                &env::var("ALLOWED_INVITERS").unwrap_or_default(),
            ),
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

fn parse_allowed_inviters(value: &str) -> HashSet<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
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
            env::remove_var("ALLOWED_INVITERS");
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
        assert!(config.allowed_inviters.is_empty());
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

    #[test]
    fn allowed_inviter_parser_trims_and_deduplicates_entries() {
        let inviters =
            parse_allowed_inviters(" @alice:example.org, ,@bob:example.org,@alice:example.org ");

        assert_eq!(inviters.len(), 2);
        assert!(inviters.contains("@alice:example.org"));
        assert!(inviters.contains("@bob:example.org"));
    }
}
