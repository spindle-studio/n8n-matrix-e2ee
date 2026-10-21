pub(crate) mod device;
pub(crate) mod media_cache;
pub(crate) mod session;
pub(crate) mod sync;

use std::{collections::HashMap, path::Path, sync::Arc};

use matrix_sdk::Client;
use tokio::sync::broadcast;
use tracing::{error, info};

use crate::{
    api::GatewayEvent,
    config::{AccountConfig, Config},
};

use self::media_cache::MediaCache;

#[derive(Clone)]
pub(crate) struct AccountState {
    pub(crate) account_id: String,
    pub(crate) client: Client,
    pub(crate) events: broadcast::Sender<GatewayEvent>,
    pub(crate) api_secret_token: String,
    pub(crate) media_cache: MediaCache,
}

#[derive(Clone)]
pub(crate) struct AppState {
    accounts_by_token: Arc<HashMap<String, AccountState>>,
}

impl AppState {
    pub(crate) fn from_accounts(accounts: impl IntoIterator<Item = AccountState>) -> Self {
        let accounts_by_token = accounts
            .into_iter()
            .map(|account| (account.api_secret_token.clone(), account))
            .collect();
        Self {
            accounts_by_token: Arc::new(accounts_by_token),
        }
    }

    pub(crate) fn account_for_token(&self, token: &str) -> Option<&AccountState> {
        self.accounts_by_token.get(token)
    }
}

pub(crate) async fn initialize_gateway(config: &Config) -> anyhow::Result<AppState> {
    let mut initialized_accounts = Vec::with_capacity(config.accounts.len());

    for account in &config.accounts {
        let account_data_dir = config.account_data_dir(&account.id);
        match initialize_account(&account_data_dir, account).await {
            Ok(account_state) => initialized_accounts.push(account_state),
            Err(error) => {
                error!(
                    account = %account.id,
                    user = %account.matrix_user,
                    %error,
                    "failed to initialize Matrix account"
                );
            }
        }
    }

    if initialized_accounts.is_empty() {
        anyhow::bail!("failed to initialize any configured Matrix accounts");
    }

    Ok(AppState::from_accounts(initialized_accounts))
}

async fn initialize_account(
    account_data_dir: &Path,
    account: &AccountConfig,
) -> anyhow::Result<AccountState> {
    let store_path = session::matrix_store_path(account_data_dir);
    let session_path = session::matrix_session_path(account_data_dir);
    let media_cache_path = session::media_cache_path(account_data_dir);
    std::fs::create_dir_all(&store_path)?;
    let media_cache = MediaCache::open(&media_cache_path)?;

    if !session_path.exists() && session::store_contains_data(&store_path)? {
        anyhow::bail!(
            "Matrix store exists without its session file; remove the persisted gateway data once \
             to create a recoverable Matrix session"
        );
    }

    let client = Client::builder()
        .homeserver_url(&account.homeserver_url)
        .sqlite_store(&store_path, None)
        .build()
        .await?;

    if session_path.exists() {
        let session = session::load_session(&session_path)?;

        if session.meta.user_id.as_str() != account.matrix_user {
            anyhow::bail!(
                "persisted Matrix session is for {}, but account '{}' matrix_user is {}",
                session.meta.user_id,
                account.id,
                account.matrix_user
            );
        }

        client.restore_session(session).await?;
        info!(
            account = %account.id,
            user = %account.matrix_user,
            "restored persisted Matrix session"
        );
    } else {
        client
            .matrix_auth()
            .login_username(&account.matrix_user, &account.matrix_password)
            .initial_device_display_name("n8n Matrix E2EE gateway")
            .send()
            .await?;
        session::persist_session(&session_path, client.matrix_auth().session())?;
        info!(
            account = %account.id,
            user = %account.matrix_user,
            "logged in to Matrix"
        );
    }

    device::ensure_cross_signing_bootstrap(&account.id, &account.matrix_password, &client).await?;
    let own_user_id = client
        .user_id()
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow::anyhow!("logged-in Matrix client has no user id"))?;

    let (events, _) = broadcast::channel(256);
    sync::register_event_handlers(&account.id, &client, events.clone(), own_user_id);

    let sync_client = client.clone();
    let sync_account_id = account.id.clone();
    tokio::spawn(async move {
        sync::run_sync_loop(sync_account_id, sync_client).await;
    });

    Ok(AccountState {
        account_id: account.id.clone(),
        client,
        events,
        api_secret_token: account.api_secret_token.clone(),
        media_cache,
    })
}
