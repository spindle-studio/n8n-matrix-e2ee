use std::time::Duration;

use axum::{Json, extract::State};
use matrix_sdk::ruma::api::client::typing::create_typing_event::v3::{
    Request as MatrixTypingRequest, Typing, TypingInfo,
};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::gateway::AppState;

use super::{
    auth::authorize_request,
    error::{ApiResult, client_error, server_error},
    room::resolve_known_room,
};

const DEFAULT_TYPING_TIMEOUT_SECONDS: u64 = 30;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TypingRequest {
    room_id: String,
    typing: bool,
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RoomTypingResponse {
    room_id: String,
    typing: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout_seconds: Option<u64>,
}

fn resolve_typing_state(typing: bool, timeout_seconds: Option<u64>) -> ApiResult<(Typing, Option<u64>)> {
    if !typing {
        return Ok((Typing::No, None));
    }

    let seconds = timeout_seconds.unwrap_or(DEFAULT_TYPING_TIMEOUT_SECONDS);
    if seconds == 0 {
        return Err(client_error("timeoutSeconds must be greater than 0"));
    }

    Ok((
        Typing::Yes(TypingInfo::new(Duration::from_secs(seconds))),
        Some(seconds),
    ))
}

pub(super) async fn set_typing(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<TypingRequest>,
) -> ApiResult<Json<RoomTypingResponse>> {
    let account = authorize_request(&state, &headers)?;
    let (typing_state, resolved_timeout_seconds) =
        resolve_typing_state(request.typing, request.timeout_seconds)?;
    let room = resolve_known_room(&account.client, &request.room_id).await?;
    let room_id = room.room_id().to_owned();

    info!(
        account = %account.account_id,
        room_id = %room_id,
        typing = request.typing,
        timeout_seconds = resolved_timeout_seconds,
        "updating Matrix typing state"
    );
    let matrix_request = MatrixTypingRequest::new(
        room.own_user_id().to_owned(),
        room.room_id().to_owned(),
        typing_state,
    );
    account.client.send(matrix_request).await.map_err(|error| {
        error!(account = %account.account_id, %error, room_id = %room_id, typing = request.typing, "failed to update Matrix typing state");
        server_error("failed to update Matrix typing state")
    })?;

    Ok(Json(RoomTypingResponse {
        room_id: room_id.to_string(),
        typing: request.typing,
        timeout_seconds: resolved_timeout_seconds,
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
                typing: true,
                timeout_seconds: Some(30),
            })
            .unwrap(),
            serde_json::json!({"roomId": "!room:example.org", "typing": true, "timeoutSeconds": 30})
        );
        assert_eq!(
            serde_json::to_value(RoomTypingResponse {
                room_id: "!room:example.org".to_owned(),
                typing: false,
                timeout_seconds: None,
            })
            .unwrap(),
            serde_json::json!({"roomId": "!room:example.org", "typing": false})
        );
    }

    #[test]
    fn resolve_typing_state_uses_default_or_custom_timeout_and_rejects_zero() {
        let (state, seconds) = resolve_typing_state(true, None).unwrap();
        assert!(matches!(state, Typing::Yes(info) if info.timeout == Duration::from_secs(30)));
        assert_eq!(seconds, Some(30));

        let (state, seconds) = resolve_typing_state(true, Some(15)).unwrap();
        assert!(matches!(state, Typing::Yes(info) if info.timeout == Duration::from_secs(15)));
        assert_eq!(seconds, Some(15));

        assert!(resolve_typing_state(true, Some(0)).is_err());

        let (state, seconds) = resolve_typing_state(false, Some(15)).unwrap();
        assert!(matches!(state, Typing::No));
        assert_eq!(seconds, None);
    }
}
