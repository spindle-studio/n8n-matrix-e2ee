use axum::{Json, extract::State, http::HeaderMap};
use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
use serde::Deserialize;
use tracing::{error, info};

use crate::gateway::AppState;

use super::{MessageActionResponse, event::GatewayMessageType, message_action_response};
use crate::api::{
    auth::authorize_request,
    error::{ApiResult, client_error, server_error},
    room::{get_known_room, parse_room_id},
};

#[derive(Debug, Deserialize)]
pub(super) struct SendTextRequest {
    room_id: String,
    message: String,
}

pub(super) async fn send_text(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SendTextRequest>,
) -> ApiResult<Json<MessageActionResponse>> {
    authorize_request(&state.api_secret_token, &headers)?;
    if request.message.trim().is_empty() {
        return Err(client_error("message must not be empty"));
    }

    let room_id = parse_room_id(&request.room_id)?;
    let room = get_known_room(&state.client, &room_id)?;
    info!(room_id = %room_id, message_type = "text", "sending Matrix message");
    let response = room
        .send(RoomMessageEventContent::text_plain(request.message))
        .await
        .map_err(|error| {
            error!(%error, room_id = %room_id, "failed to send Matrix message");
            server_error("failed to send Matrix message")
        })?;

    Ok(message_action_response(
        room_id.to_string(),
        response.response.event_id.to_string(),
        GatewayMessageType::Text,
    ))
}
