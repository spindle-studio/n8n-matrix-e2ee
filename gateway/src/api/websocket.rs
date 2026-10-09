use std::net::SocketAddr;

use axum::{
    extract::{
        ConnectInfo, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::HeaderMap,
    response::IntoResponse,
};
use tokio::sync::broadcast;
use tracing::{error, info, warn};

use crate::gateway::AppState;

use super::{GatewayEvent, auth::authorize_request};

pub(crate) async fn websocket(
    ws: WebSocketUpgrade,
    ConnectInfo(remote_addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let user_agent = request_user_agent(&headers);
    if let Err(error) = authorize_request(&state.api_secret_token, &headers) {
        warn!(remote_addr = %remote_addr, user_agent, "rejecting unauthorized WebSocket connection");
        return error.into_response();
    }
    ws.on_upgrade(move |socket| {
        forward_events(socket, state.events.subscribe(), remote_addr, user_agent)
    })
}

fn request_user_agent(headers: &HeaderMap) -> String {
    headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("unknown")
        .to_owned()
}

async fn forward_events(
    mut socket: WebSocket,
    mut events: broadcast::Receiver<GatewayEvent>,
    remote_addr: SocketAddr,
    user_agent: String,
) {
    info!(remote_addr = %remote_addr, user_agent, "WebSocket client connected to Matrix event stream");
    loop {
        match events.recv().await {
            Ok(event) => match serde_json::to_string(&event) {
                Ok(payload) => {
                    if socket.send(Message::Text(payload.into())).await.is_err() {
                        return;
                    }
                }
                Err(error) => error!(%error, "failed to serialize Matrix event"),
            },
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                warn!(skipped, remote_addr = %remote_addr, user_agent, "WebSocket client lagged behind Matrix event stream")
            }
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}
