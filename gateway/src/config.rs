use std::{
    collections::{BTreeMap, HashMap},
    env,
    path::{Component, Path, PathBuf},
};

use clap::Parser;
use serde::Deserialize;
use thiserror::Error;

pub(crate) const DEFAULT_PORT: u16 = 8080;
pub(crate) const DEFAULT_CONFIG_PATH: &str = "./config.toml";
pub(crate) const DEFAULT_DATA_DIR: &str = "./data";

#[derive(Debug, Clone, Parser)]
#[command(name = "n8n-matrix-e2ee-gateway", about = "Matrix E2EE gateway for n8n")]
pub(crate) struct CliArgs {
    /// Path to the gateway TOML configuration file
    #[arg(long = "config", env = "GATEWAY_CONFIG", default_value = DEFAULT_CONFIG_PATH)]
    pub(crate) config: PathBuf,

    /// Directory for persisted Matrix sessions, encryption stores, and media cache
    #[arg(long = "data-dir", env = "GATEWAY_DATA_DIR")]
    pub(crate) data_dir: Option<PathBuf>,

    /// TCP port to listen on
    #[arg(long = "port", env = "GATEWAY_PORT")]
    pub(crate) port: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AccountConfig {
    pub(crate) id: String,
    pub(crate) homeserver_url: String,
    pub(crate) matrix_user: String,
    pub(crate) matrix_password: String,
    pub(crate) api_secret_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Config {
    pub(crate) port: u16,
    pub(crate) data_dir: PathBuf,
    pub(crate) accounts: Vec<AccountConfig>,
}

#[derive(Debug, Deserialize)]
struct RawConfigFile {
    port: Option<u16>,
    data_dir: Option<String>,
    #[serde(flatten)]
    accounts: BTreeMap<String, RawAccountConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAccountConfig {
    homeserver_url: String,
    matrix_user: String,
    matrix_password: String,
    api_secret_token: String,
}

#[derive(Debug, Error)]
pub(crate) enum ConfigError {
    #[error("failed to read configuration file {path}: {source}")]
    ReadConfigFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse TOML configuration in {path}: {source}")]
    ParseToml {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("environment variable {0} referenced in configuration must be set and non-empty")]
    MissingEnvironmentVariable(String),
    #[error("invalid environment variable interpolation in {field}: {reason}")]
    InvalidInterpolation {
        field: String,
        reason: &'static str,
    },
    #[error("at least one Matrix account table must be configured")]
    NoAccountsConfigured,
    #[error("account name '{0}' is invalid; it must be a single safe directory name")]
    InvalidAccountId(String),
    #[error("account '{account}' field '{field}' must not be empty")]
    EmptyAccountField {
        account: String,
        field: &'static str,
    },
    #[error("accounts '{first_account}' and '{second_account}' must not share the same api_secret_token")]
    DuplicateApiToken {
        first_account: String,
        second_account: String,
    },
    #[error("accounts '{first_account}' and '{second_account}' configure the same matrix_user '{matrix_user}'")]
    DuplicateMatrixUser {
        matrix_user: String,
        first_account: String,
        second_account: String,
    },
}

impl Config {
    pub(crate) fn load() -> Result<Self, ConfigError> {
        let args = CliArgs::parse();
        Self::from_args(args, |name| env::var(name).ok())
    }

    pub(crate) fn from_args<F>(args: CliArgs, env_lookup: F) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let contents = std::fs::read_to_string(&args.config).map_err(|source| {
            ConfigError::ReadConfigFile {
                path: args.config.clone(),
                source,
            }
        })?;
        Self::from_toml_str(&contents, &args.config, args.data_dir, args.port, env_lookup)
    }

    pub(crate) fn from_toml_str<F>(
        contents: &str,
        config_path: &Path,
        cli_data_dir: Option<PathBuf>,
        cli_port: Option<u16>,
        env_lookup: F,
    ) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let raw: RawConfigFile =
            toml::from_str(contents).map_err(|source| ConfigError::ParseToml {
                path: config_path.to_path_buf(),
                source,
            })?;

        let port = cli_port.or(raw.port).unwrap_or(DEFAULT_PORT);

        let cli_data_dir = cli_data_dir
            .and_then(|path| {
                let trimmed = path.to_string_lossy().trim().to_owned();
                (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
            });
        let toml_data_dir = match raw.data_dir {
            Some(raw_dir) => {
                let interpolated = interpolate_env_vars(&raw_dir, "data_dir", &env_lookup)?;
                let trimmed = interpolated.trim();
                (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
            }
            None => None,
        };
        let data_dir = cli_data_dir
            .or(toml_data_dir)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR));

        if raw.accounts.is_empty() {
            return Err(ConfigError::NoAccountsConfigured);
        }

        let mut accounts = Vec::with_capacity(raw.accounts.len());
        let mut seen_tokens: HashMap<String, String> = HashMap::new();
        let mut seen_users: HashMap<String, String> = HashMap::new();

        for (account_id, raw_account) in raw.accounts {
            validate_account_id(&account_id)?;

            let homeserver_url = resolve_account_field(
                &account_id,
                "homeserver_url",
                &raw_account.homeserver_url,
                true,
                &env_lookup,
            )?;
            let matrix_user = resolve_account_field(
                &account_id,
                "matrix_user",
                &raw_account.matrix_user,
                true,
                &env_lookup,
            )?;
            let matrix_password = resolve_account_field(
                &account_id,
                "matrix_password",
                &raw_account.matrix_password,
                false,
                &env_lookup,
            )?;
            let api_secret_token = resolve_account_field(
                &account_id,
                "api_secret_token",
                &raw_account.api_secret_token,
                true,
                &env_lookup,
            )?;

            if let Some(existing_account) =
                seen_tokens.insert(api_secret_token.clone(), account_id.clone())
            {
                return Err(ConfigError::DuplicateApiToken {
                    first_account: existing_account,
                    second_account: account_id,
                });
            }

            if let Some(existing_account) =
                seen_users.insert(matrix_user.clone(), account_id.clone())
            {
                return Err(ConfigError::DuplicateMatrixUser {
                    matrix_user,
                    first_account: existing_account,
                    second_account: account_id,
                });
            }

            accounts.push(AccountConfig {
                id: account_id,
                homeserver_url,
                matrix_user,
                matrix_password,
                api_secret_token,
            });
        }

        Ok(Self {
            port,
            data_dir,
            accounts,
        })
    }

    pub(crate) fn account_data_dir(&self, account_id: &str) -> PathBuf {
        self.data_dir.join(account_id)
    }
}

fn validate_account_id(account_id: &str) -> Result<(), ConfigError> {
    if account_id.is_empty()
        || account_id.trim() != account_id
        || account_id.contains('/')
        || account_id.contains('\\')
        || account_id.contains('\0')
    {
        return Err(ConfigError::InvalidAccountId(account_id.to_owned()));
    }

    let mut components = Path::new(account_id).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(()),
        _ => Err(ConfigError::InvalidAccountId(account_id.to_owned())),
    }
}

fn resolve_account_field<F>(
    account: &str,
    field: &'static str,
    raw_value: &str,
    trim_value: bool,
    env_lookup: &F,
) -> Result<String, ConfigError>
where
    F: Fn(&str) -> Option<String>,
{
    let field_context = format!("{account}.{field}");
    let interpolated = interpolate_env_vars(raw_value, &field_context, env_lookup)?;
    let normalized = if trim_value {
        interpolated.trim().to_owned()
    } else {
        interpolated
    };

    if normalized.trim().is_empty() {
        return Err(ConfigError::EmptyAccountField {
            account: account.to_owned(),
            field,
        });
    }

    Ok(normalized)
}

pub(crate) fn interpolate_env_vars<F>(
    input: &str,
    field: &str,
    env_lookup: &F,
) -> Result<String, ConfigError>
where
    F: Fn(&str) -> Option<String>,
{
    let mut output = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(dollar_idx) = rest.find('$') {
        output.push_str(&rest[..dollar_idx]);
        let after_dollar = &rest[dollar_idx + 1..];

        if let Some(after_escaped) = after_dollar.strip_prefix("${") {
            output.push_str("${");
            rest = after_escaped;
            continue;
        }

        if let Some(after_open) = after_dollar.strip_prefix('{') {
            let Some(close_idx) = after_open.find('}') else {
                return Err(ConfigError::InvalidInterpolation {
                    field: field.to_owned(),
                    reason: "unclosed '${' placeholder",
                });
            };
            let var_name = &after_open[..close_idx];
            if !is_valid_env_var_name(var_name) {
                return Err(ConfigError::InvalidInterpolation {
                    field: field.to_owned(),
                    reason: "environment variable name in '${...}' must be non-empty ASCII alphanumeric or '_'",
                });
            }
            let value = env_lookup(var_name)
                .filter(|val| !val.trim().is_empty())
                .ok_or_else(|| ConfigError::MissingEnvironmentVariable(var_name.to_owned()))?;
            output.push_str(&value);
            rest = &after_open[close_idx + 1..];
        } else {
            output.push('$');
            rest = after_dollar;
        }
    }

    output.push_str(rest);
    Ok(output)
}

fn is_valid_env_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multi_account_toml_with_env_interpolation() {
        let toml = r#"
port = 9090
data_dir = "${CUSTOM_DATA_DIR}/state"

[support-bot]
homeserver_url = "https://matrix.example.org"
matrix_user = "@support:example.org"
matrix_password = "${SUPPORT_PASSWORD}"
api_secret_token = "${SUPPORT_TOKEN}"

[alerts-bot]
homeserver_url = "https://alerts.example.org"
matrix_user = "@alerts:example.org"
matrix_password = "literal-password-with-$sign-and-$${ESCAPED}"
api_secret_token = "literal-alerts-token"
"#;

        let env = HashMap::from([
            ("CUSTOM_DATA_DIR", "/var/lib/gateway".to_owned()),
            ("SUPPORT_PASSWORD", "s3cr3t-pass".to_owned()),
            ("SUPPORT_TOKEN", "support-token-123".to_owned()),
        ]);

        let config = Config::from_toml_str(toml, Path::new("config.toml"), None, None, |key| {
            env.get(key).cloned()
        })
        .expect("config should parse");

        assert_eq!(config.port, 9090);
        assert_eq!(config.data_dir, PathBuf::from("/var/lib/gateway/state"));
        assert_eq!(config.accounts.len(), 2);

        assert_eq!(
            config.accounts[0],
            AccountConfig {
                id: "alerts-bot".to_owned(),
                homeserver_url: "https://alerts.example.org".to_owned(),
                matrix_user: "@alerts:example.org".to_owned(),
                matrix_password: "literal-password-with-$sign-and-${ESCAPED}".to_owned(),
                api_secret_token: "literal-alerts-token".to_owned(),
            }
        );
        assert_eq!(
            config.accounts[1],
            AccountConfig {
                id: "support-bot".to_owned(),
                homeserver_url: "https://matrix.example.org".to_owned(),
                matrix_user: "@support:example.org".to_owned(),
                matrix_password: "s3cr3t-pass".to_owned(),
                api_secret_token: "support-token-123".to_owned(),
            }
        );
        assert_eq!(
            config.account_data_dir("support-bot"),
            PathBuf::from("/var/lib/gateway/state/support-bot")
        );
    }

    #[test]
    fn cli_args_parse_flags_and_defaults() {
        let default_args = CliArgs::try_parse_from(["n8n-matrix-e2ee-gateway"]).unwrap();
        assert_eq!(default_args.config, PathBuf::from("./config.toml"));
        assert_eq!(default_args.data_dir, None);
        assert_eq!(default_args.port, None);

        let custom_args = CliArgs::try_parse_from([
            "n8n-matrix-e2ee-gateway",
            "--config",
            "/etc/gateway/config.toml",
            "--data-dir",
            "/var/lib/gateway",
            "--port",
            "9000",
        ])
        .unwrap();
        assert_eq!(
            custom_args.config,
            PathBuf::from("/etc/gateway/config.toml")
        );
        assert_eq!(custom_args.data_dir, Some(PathBuf::from("/var/lib/gateway")));
        assert_eq!(custom_args.port, Some(9000));
    }

    #[test]
    fn cli_data_dir_and_port_override_toml_and_defaults() {
        let toml = r#"
[bot]
homeserver_url = "https://matrix.example.org"
matrix_user = "@bot:example.org"
matrix_password = "secret"
api_secret_token = "token"
"#;

        let default_config =
            Config::from_toml_str(toml, Path::new("config.toml"), None, None, |_| None).unwrap();
        assert_eq!(default_config.port, DEFAULT_PORT);
        assert_eq!(default_config.data_dir, PathBuf::from(DEFAULT_DATA_DIR));

        let overridden = Config::from_toml_str(
            toml,
            Path::new("config.toml"),
            Some(PathBuf::from("/custom/data")),
            Some(8181),
            |_| None,
        )
        .unwrap();
        assert_eq!(overridden.port, 8181);
        assert_eq!(overridden.data_dir, PathBuf::from("/custom/data"));
    }

    #[test]
    fn rejects_duplicate_tokens_and_duplicate_users() {
        let dup_token_toml = r#"
[bot-a]
homeserver_url = "https://matrix.example.org"
matrix_user = "@bot-a:example.org"
matrix_password = "secret"
api_secret_token = "shared-token"

[bot-b]
homeserver_url = "https://matrix.example.org"
matrix_user = "@bot-b:example.org"
matrix_password = "secret"
api_secret_token = "shared-token"
"#;
        assert!(matches!(
            Config::from_toml_str(dup_token_toml, Path::new("config.toml"), None, None, |_| None),
            Err(ConfigError::DuplicateApiToken { .. })
        ));

        let dup_user_toml = r#"
[bot-a]
homeserver_url = "https://matrix.example.org"
matrix_user = "@same:example.org"
matrix_password = "secret"
api_secret_token = "token-a"

[bot-b]
homeserver_url = "https://matrix.example.org"
matrix_user = "@same:example.org"
matrix_password = "secret"
api_secret_token = "token-b"
"#;
        assert!(matches!(
            Config::from_toml_str(dup_user_toml, Path::new("config.toml"), None, None, |_| None),
            Err(ConfigError::DuplicateMatrixUser { .. })
        ));
    }

    #[test]
    fn rejects_missing_env_vars_invalid_account_ids_and_empty_accounts() {
        assert!(matches!(
            Config::from_toml_str("port = 8080\n", Path::new("config.toml"), None, None, |_| None),
            Err(ConfigError::NoAccountsConfigured)
        ));

        let missing_env_toml = r#"
[bot]
homeserver_url = "https://matrix.example.org"
matrix_user = "@bot:example.org"
matrix_password = "${MISSING_PASSWORD}"
api_secret_token = "token"
"#;
        assert!(matches!(
            Config::from_toml_str(missing_env_toml, Path::new("config.toml"), None, None, |_| None),
            Err(ConfigError::MissingEnvironmentVariable(var)) if var == "MISSING_PASSWORD"
        ));

        let bad_id_toml = r#"
["../escape"]
homeserver_url = "https://matrix.example.org"
matrix_user = "@bot:example.org"
matrix_password = "secret"
api_secret_token = "token"
"#;
        assert!(matches!(
            Config::from_toml_str(bad_id_toml, Path::new("config.toml"), None, None, |_| None),
            Err(ConfigError::InvalidAccountId(_))
        ));

        let unclosed_toml = r#"
[bot]
homeserver_url = "https://matrix.example.org"
matrix_user = "@bot:example.org"
matrix_password = "${UNCLOSED"
api_secret_token = "token"
"#;
        assert!(matches!(
            Config::from_toml_str(unclosed_toml, Path::new("config.toml"), None, None, |_| None),
            Err(ConfigError::InvalidInterpolation { .. })
        ));
    }

    #[test]
    fn example_config_file_parses() {
        let example_toml = include_str!("../config.toml.example");

        let config =
            Config::from_toml_str(example_toml, Path::new("config.toml.example"), None, None, |_| {
                None
            })
            .expect("config.toml.example should parse");

        assert_eq!(config.port, 8080);
        assert_eq!(config.data_dir, PathBuf::from("./data"));
        assert_eq!(config.accounts.len(), 1);
        assert_eq!(config.accounts[0].id, "default");
    }
}

