use axum::{Json, extract::State, http::HeaderMap};
use matrix_sdk::ruma::OwnedEventId;
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
#[serde(rename_all = "camelCase")]
pub(super) struct RedactEventRequest {
    room_id: String,
    target_event_id: String,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RedactActionResponse {
    room_id: String,
    event_id: String,
    target_event_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

pub(super) async fn redact_event(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RedactEventRequest>,
) -> ApiResult<Json<RedactActionResponse>> {
    let account = authorize_request(&state, &headers)?;
    let (target_event_id, reason) =
        validate_redact_request(&request.target_event_id, request.reason.as_deref())?;

    let room = resolve_known_room(&account.client, &request.room_id).await?;
    let room_id = room.room_id().to_owned();
    info!(
        account = %account.account_id,
        room_id = %room_id,
        target_event_id = %target_event_id,
        has_reason = reason.is_some(),
        "redacting Matrix event"
    );
    let response = room
        .redact(&target_event_id, reason.as_deref(), None)
        .await
        .map_err(|error| {
            error!(
                account = %account.account_id,
                %error,
                room_id = %room_id,
                target_event_id = %target_event_id,
                "failed to redact Matrix event"
            );
            server_error("failed to redact Matrix event")
        })?;

    Ok(Json(RedactActionResponse {
        room_id: room_id.to_string(),
        event_id: response.event_id.to_string(),
        target_event_id: target_event_id.to_string(),
        reason,
    }))
}

fn validate_redact_request(
    event_id: &str,
    reason: Option<&str>,
) -> ApiResult<(OwnedEventId, Option<String>)> {
    let trimmed_event_id = event_id.trim();
    if trimmed_event_id.is_empty() {
        return Err(client_error("targetEventId must not be empty"));
    }
    let parsed_event_id: OwnedEventId = trimmed_event_id
        .parse()
        .map_err(|_| client_error("targetEventId must be a valid Matrix event ID"))?;

    let normalized_reason = reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    Ok((parsed_event_id, normalized_reason))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_trims_and_normalizes_redact_event_id_and_reason() {
        let (event_id, reason) =
            validate_redact_request(" $target:example.org ", Some("  Spam message  ")).unwrap();
        assert_eq!(event_id.as_str(), "$target:example.org");
        assert_eq!(reason.as_deref(), Some("Spam message"));

        let (_, empty_reason) = validate_redact_request("$target:example.org", Some("   ")).unwrap();
        assert_eq!(empty_reason, None);

        let (_, none_reason) = validate_redact_request("$target:example.org", None).unwrap();
        assert_eq!(none_reason, None);
    }

    #[test]
    fn rejects_empty_or_invalid_redact_event_id() {
        assert!(validate_redact_request("", None).is_err());
        assert!(validate_redact_request("   ", Some("reason")).is_err());
        assert!(validate_redact_request("not-an-event-id", None).is_err());
    }

    #[test]
    fn redact_response_serializes_expected_fields() {
        assert_eq!(
            serde_json::to_value(RedactActionResponse {
                room_id: "!room:example.org".to_owned(),
                event_id: "$redaction:example.org".to_owned(),
                target_event_id: "$target:example.org".to_owned(),
                reason: Some("Spam".to_owned()),
            })
            .unwrap(),
            serde_json::json!({
                "roomId": "!room:example.org",
                "eventId": "$redaction:example.org",
                "targetEventId": "$target:example.org",
                "reason": "Spam"
            })
        );

        assert_eq!(
            serde_json::to_value(RedactActionResponse {
                room_id: "!room:example.org".to_owned(),
                event_id: "$redaction:example.org".to_owned(),
                target_event_id: "$target:example.org".to_owned(),
                reason: None,
            })
            .unwrap(),
            serde_json::json!({
                "roomId": "!room:example.org",
                "eventId": "$redaction:example.org",
                "targetEventId": "$target:example.org"
            })
        );
    }
}
