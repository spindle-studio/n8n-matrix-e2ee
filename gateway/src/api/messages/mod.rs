mod audio;
mod event;
mod image;
mod text;

use axum::{Json, Router, routing::post};
use serde::Serialize;

use crate::gateway::AppState;

pub(crate) use event::{
    GatewayEvent, gateway_message_type_label, room_message_content_to_gateway_event,
};

#[derive(Debug, Serialize)]
pub(crate) struct MessageActionResponse {
    room_id: String,
    event_id: String,
    message_type: event::GatewayMessageType,
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/text", post(text::send_text))
        .route("/image", post(image::send_image))
        .route("/audio", post(audio::send_audio))
}

fn message_action_response(
    room_id: String,
    event_id: String,
    message_type: event::GatewayMessageType,
) -> Json<MessageActionResponse> {
    Json(MessageActionResponse {
        room_id,
        event_id,
        message_type,
    })
}
