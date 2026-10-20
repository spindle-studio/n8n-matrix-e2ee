use axum::{Json, extract::State, http::HeaderMap};
use matrix_sdk::ruma::{
    OwnedEventId,
    events::{reaction::ReactionEventContent, relation::Annotation},
};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::{
    api::{
        auth::authorize_request,
        error::{ApiResult, client_error, server_error},
        room::resolve_known_room,
    },
    gateway::AppState,
};

#[derive(Debug, Deserialize)]
pub(super) struct SendReactionRequest {
    room_id: String,
    event_id: String,
    key: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct ReactionActionResponse {
    room_id: String,
    event_id: String,
    target_event_id: String,
    key: String,
}

pub(super) async fn send_reaction(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SendReactionRequest>,
) -> ApiResult<Json<ReactionActionResponse>> {
    authorize_request(&state.api_secret_token, &headers)?;
    let (target_event_id, key) = validate_reaction_request(&request.event_id, &request.key)?;
    let content = ReactionEventContent::new(Annotation::new(target_event_id.clone(), key.clone()));

    let room = resolve_known_room(&state.client, &request.room_id).await?;
    let room_id = room.room_id().to_owned();
    info!(
        room_id = %room_id,
        target_event_id = %target_event_id,
        "sending Matrix reaction"
    );
    let response = room.send(content).await.map_err(|error| {
        error!(
            %error,
            room_id = %room_id,
            target_event_id = %target_event_id,
            "failed to send Matrix reaction"
        );
        server_error("failed to send Matrix reaction")
    })?;

    Ok(Json(ReactionActionResponse {
        room_id: room_id.to_string(),
        event_id: response.response.event_id.to_string(),
        target_event_id: target_event_id.to_string(),
        key,
    }))
}

fn validate_reaction_request(event_id: &str, key: &str) -> ApiResult<(OwnedEventId, String)> {
    let trimmed_event_id = event_id.trim();
    if trimmed_event_id.is_empty() {
        return Err(client_error("event_id must not be empty"));
    }
    let parsed_event_id: OwnedEventId = trimmed_event_id
        .parse()
        .map_err(|_| client_error("event_id must be a valid Matrix event ID"))?;

    let trimmed_key = key.trim();
    if trimmed_key.is_empty() {
        return Err(client_error("key must not be empty"));
    }

    Ok((parsed_event_id, trimmed_key.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_and_trims_reaction_event_id_and_key() {
        let (event_id, key) = validate_reaction_request(" $target:example.org ", " 👍 ").unwrap();
        assert_eq!(event_id.as_str(), "$target:example.org");
        assert_eq!(key, "👍");

        let content = ReactionEventContent::new(Annotation::new(event_id, key));
        assert_eq!(
            serde_json::to_value(content).unwrap(),
            serde_json::json!({
                "m.relates_to": {
                    "rel_type": "m.annotation",
                    "event_id": "$target:example.org",
                    "key": "👍"
                }
            })
        );
    }

    #[test]
    fn rejects_empty_or_invalid_reaction_inputs() {
        assert!(validate_reaction_request("   ", "👍").is_err());
        assert!(validate_reaction_request("not-an-event-id", "👍").is_err());
        assert!(validate_reaction_request("$target:example.org", "").is_err());
        assert!(validate_reaction_request("$target:example.org", "   ").is_err());
    }
}
