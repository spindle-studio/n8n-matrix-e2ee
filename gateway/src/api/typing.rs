use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::gateway::AppState;

use super::{
    auth::authorize_request,
    error::{ApiResult, server_error},
    room::{get_known_room, parse_room_id},
};

#[derive(Debug, Deserialize)]
pub(super) struct TypingRequest {
    room_id: String,
    typing: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct RoomTypingResponse {
    room_id: String,
    typing: bool,
}

pub(super) async fn set_typing(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<TypingRequest>,
) -> ApiResult<Json<RoomTypingResponse>> {
    authorize_request(&state.api_secret_token, &headers)?;
    let room_id = parse_room_id(&request.room_id)?;
    let room = get_known_room(&state.client, &room_id)?;

    info!(room_id = %room_id, typing = request.typing, "updating Matrix typing state");
    room.typing_notice(request.typing).await.map_err(|error| {
        error!(%error, room_id = %room_id, typing = request.typing, "failed to update Matrix typing state");
        server_error("failed to update Matrix typing state")
    })?;

    Ok(Json(RoomTypingResponse {
        room_id: room_id.to_string(),
        typing: request.typing,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_response_serializes_expected_fields() {
        assert_eq!(
            serde_json::to_value(RoomTypingResponse {
                room_id: "!room:example.org".to_owned(),
                typing: true
            })
            .unwrap(),
            serde_json::json!({"room_id": "!room:example.org", "typing": true})
        );
    }
}
