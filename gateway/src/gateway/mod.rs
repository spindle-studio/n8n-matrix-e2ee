pub(crate) mod device;
pub(crate) mod session;
pub(crate) mod sync;

use matrix_sdk::Client;
use tokio::sync::broadcast;
use tracing::info;

use crate::{api::GatewayEvent, config::Config};

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) client: Client,
    pub(crate) events: broadcast::Sender<GatewayEvent>,
    pub(crate) api_secret_token: String,
}

pub(crate) async fn initialize_gateway(config: &Config) -> anyhow::Result<AppState> {
    let store_path = session::matrix_store_path(&config.data_dir);
    let session_path = session::matrix_session_path(&config.data_dir);
    std::fs::create_dir_all(&store_path)?;

    if !session_path.exists() && session::store_contains_data(&store_path)? {
        anyhow::bail!(
            "Matrix store exists without its session file; remove the persisted gateway data once \
             to create a recoverable Matrix session"
        );
    }

    let client = Client::builder()
        .homeserver_url(&config.homeserver_url)
        .sqlite_store(&store_path, None)
        .build()
        .await?;

    if session_path.exists() {
        let session = session::load_session(&session_path)?;

        if session.meta.user_id.as_str() != config.matrix_user {
            anyhow::bail!(
                "persisted Matrix session is for {}, but MATRIX_USER is {}",
                session.meta.user_id,
                config.matrix_user
            );
        }

        client.restore_session(session).await?;
        info!(user = %config.matrix_user, "restored persisted Matrix session");
    } else {
        client
            .matrix_auth()
            .login_username(&config.matrix_user, &config.matrix_password)
            .initial_device_display_name("n8n Matrix E2EE gateway")
            .send()
            .await?;
        session::persist_session(&session_path, client.matrix_auth().session())?;
        info!(user = %config.matrix_user, "logged in to Matrix");
    }

    device::ensure_cross_signing_bootstrap(config, &client).await?;
    let own_user_id = client
        .user_id()
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow::anyhow!("logged-in Matrix client has no user id"))?;

    let (events, _) = broadcast::channel(256);
    sync::register_event_handlers(&client, events.clone(), own_user_id);

    let sync_client = client.clone();
    tokio::spawn(async move {
        sync::run_sync_loop(sync_client).await;
    });

    Ok(AppState {
        client,
        events,
        api_secret_token: config.api_secret_token.clone(),
    })
}
