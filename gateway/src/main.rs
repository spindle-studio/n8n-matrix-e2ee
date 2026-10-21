#![recursion_limit = "512"]

mod api;
mod config;
mod gateway;

use std::net::SocketAddr;

use axum::{Router, routing::get};
use config::Config;
use gateway::initialize_gateway;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("n8n_matrix_e2ee_gateway=info,tower_http=info")),
        )
        .init();

    let config = Config::load().unwrap_or_else(|error| {
        eprintln!("configuration error: {error}");
        std::process::exit(2);
    });

    let state = initialize_gateway(&config).await.unwrap_or_else(|error| {
        error!(%error, "failed to initialize Matrix gateway");
        std::process::exit(1);
    });

    let app = Router::new()
        .route("/healthz", get(api::healthz))
        .nest("/v1", api::router())
        .with_state(state);
    let address = SocketAddr::from(([0, 0, 0, 0], config.port));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("configured port should be bindable");

    info!(%address, "gateway is ready");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .expect("gateway server should not fail unexpectedly");
}
