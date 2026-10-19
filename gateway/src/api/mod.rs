mod auth;
mod error;
mod health;
mod media;
mod messages;
mod room;
mod typing;
mod websocket;

use axum::{
    Router,
    routing::{get, post},
};

use crate::gateway::AppState;

pub(crate) use health::healthz;
pub(crate) use messages::{
    GatewayEvent, gateway_message_type_label, reaction_content_to_gateway_event,
    room_message_content_to_gateway_event,
};

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/auth-check", get(auth::auth_check))
        .nest("/messages", messages::router())
        .route("/typing", post(typing::set_typing))
        .route("/media", get(media::download_media))
        .route("/ws", get(websocket::websocket))
}
