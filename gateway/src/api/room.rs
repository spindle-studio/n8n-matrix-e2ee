use std::collections::BTreeSet;

use axum::{Json, Router, extract::State, http::HeaderMap, routing::post};
use matrix_sdk::{
    Client, Room,
    ruma::{
        OwnedEventId, OwnedRoomAliasId, OwnedRoomId, OwnedRoomOrAliasId, OwnedUserId,
        api::client::{
            membership::{
                invite_user::v3::{InvitationRecipient, InviteUserId, Request as InviteUserRequest},
                leave_room::v3::Request as LeaveRoomRequest,
            },
            room::{
                Visibility,
                create_room::v3::{Request as CreateRoomRequest, RoomPreset},
            },
        },
        events::{InitialStateEvent, room::encryption::RoomEncryptionEventContent},
    },
};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::gateway::AppState;

use super::{
    auth::authorize_request,
    error::{ApiResult, client_error, server_error},
};

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/join", post(join_room))
        .route("/leave", post(leave_room))
        .route("/create", post(create_room))
        .route("/invite", post(invite_room))
}

#[derive(Debug, Deserialize)]
pub(super) struct JoinRoomRequest {
    room_id: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub(super) struct RoomIdResponse {
    room_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct LeaveRoomApiRequest {
    room_id: String,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub(super) struct LeaveRoomResponse {
    success: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct InviteRoomApiRequest {
    room_id: String,
    user_ids: Vec<String>,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateRoomApiRequest {
    name: String,
    #[serde(default = "default_visibility")]
    visibility: String,
    #[serde(default)]
    topic: Option<String>,
    #[serde(default)]
    room_alias_name: Option<String>,
    #[serde(default)]
    invite: Option<Vec<String>>,
    #[serde(default = "default_is_encrypted")]
    is_encrypted: bool,
}

fn default_visibility() -> String {
    "private".to_owned()
}

fn default_is_encrypted() -> bool {
    true
}

pub(crate) fn parse_room_or_alias_id(room_id: &str) -> ApiResult<OwnedRoomOrAliasId> {
    let trimmed = room_id.trim();
    if trimmed.is_empty() {
        return Err(client_error("room_id must not be empty"));
    }

    trimmed
        .parse()
        .map_err(|_| client_error("room_id must be a valid Matrix room ID or alias"))
}

pub(crate) fn parse_event_id(event_id: &str) -> ApiResult<OwnedEventId> {
    if event_id.trim().is_empty() {
        return Err(client_error("event_id must not be empty"));
    }

    event_id
        .parse()
        .map_err(|_| client_error("event_id must be a valid Matrix event ID"))
}

pub(crate) async fn resolve_known_room(client: &Client, room_id_or_alias: &str) -> ApiResult<Room> {
    let parsed = parse_room_or_alias_id(room_id_or_alias)?;
    let room_id = match OwnedRoomId::try_from(parsed.clone()) {
        Ok(room_id) => room_id,
        Err(_) => {
            let alias = OwnedRoomAliasId::try_from(parsed)
                .map_err(|_| client_error("room_id must be a valid Matrix room ID or alias"))?;
            if let Some(room) = client.rooms().into_iter().find(|room| {
                room.canonical_alias().as_ref() == Some(&alias)
                    || room.alt_aliases().contains(&alias)
            }) {
                return Ok(room);
            }
            client
                .resolve_room_alias(&alias)
                .await
                .map_err(|error| {
                    error!(%error, room_alias = %alias, "failed to resolve Matrix room alias");
                    client_error("failed to resolve Matrix room alias")
                })?
                .room_id
        }
    };

    client
        .get_room(&room_id)
        .ok_or_else(|| client_error("gateway does not know this room"))
}

fn normalize_optional_reason(reason: Option<String>) -> Option<String> {
    reason
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn build_invite_room_requests(
    room_id: OwnedRoomId,
    raw_user_ids: Vec<String>,
    reason: Option<String>,
) -> ApiResult<Vec<InviteUserRequest>> {
    let mut user_ids = BTreeSet::new();
    for raw_user_id in raw_user_ids {
        let trimmed = raw_user_id.trim();
        if trimmed.is_empty() {
            return Err(client_error("user_ids entries must not be empty"));
        }
        let user_id: OwnedUserId = trimmed
            .parse()
            .map_err(|_| client_error("user_ids entries must be valid Matrix user IDs"))?;
        user_ids.insert(user_id);
    }

    if user_ids.is_empty() {
        return Err(client_error("user_ids must not be empty"));
    }

    let normalized_reason = normalize_optional_reason(reason);
    let requests = user_ids
        .into_iter()
        .map(|user_id| {
            let mut recipient = InviteUserId::new(user_id);
            recipient.reason = normalized_reason.clone();
            InviteUserRequest::new(room_id.clone(), InvitationRecipient::UserId(recipient))
        })
        .collect();

    Ok(requests)
}

fn build_create_room_request(request: CreateRoomApiRequest) -> ApiResult<CreateRoomRequest> {
    let name = request.name.trim().to_owned();
    if name.is_empty() {
        return Err(client_error("name must not be empty"));
    }

    let (visibility, preset) = match request.visibility.trim() {
        "private" => (Visibility::Private, RoomPreset::PrivateChat),
        "public" => (Visibility::Public, RoomPreset::PublicChat),
        _ => return Err(client_error("visibility must be 'private' or 'public'")),
    };

    let topic = request
        .topic
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());

    let room_alias_name = match request
        .room_alias_name
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    {
        Some(alias) => {
            if alias.starts_with('#') || alias.contains(':') || alias.chars().any(char::is_whitespace) {
                return Err(client_error(
                    "room_alias_name must be a valid alias localpart without '#' or ':server'",
                ));
            }
            Some(alias)
        }
        None => None,
    };

    let mut invite_set = BTreeSet::new();
    if let Some(raw_invites) = request.invite {
        for raw_user_id in raw_invites {
            let trimmed = raw_user_id.trim();
            if trimmed.is_empty() {
                return Err(client_error("invite entries must not be empty"));
            }
            let user_id: OwnedUserId = trimmed
                .parse()
                .map_err(|_| client_error("invite entries must be valid Matrix user IDs"))?;
            invite_set.insert(user_id);
        }
    }

    let mut matrix_request = CreateRoomRequest::new();
    matrix_request.name = Some(name);
    matrix_request.visibility = visibility;
    matrix_request.preset = Some(preset);
    matrix_request.topic = topic;
    matrix_request.room_alias_name = room_alias_name;
    matrix_request.invite = invite_set.into_iter().collect();

    if request.is_encrypted {
        matrix_request.initial_state = vec![
            InitialStateEvent::with_empty_state_key(
                RoomEncryptionEventContent::with_recommended_defaults(),
            )
            .to_raw_any(),
        ];
    }

    Ok(matrix_request)
}

async fn join_room(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<JoinRoomRequest>,
) -> ApiResult<Json<RoomIdResponse>> {
    authorize_request(&state.api_secret_token, &headers)?;
    let room_or_alias_id = parse_room_or_alias_id(&request.room_id)?;
    let server_names: Vec<_> = room_or_alias_id
        .server_name()
        .map(ToOwned::to_owned)
        .into_iter()
        .collect();

    info!(room_identifier = %room_or_alias_id, "joining Matrix room");
    let room = state
        .client
        .join_room_by_id_or_alias(&room_or_alias_id, &server_names)
        .await
        .map_err(|error| {
            error!(%error, room_identifier = %room_or_alias_id, "failed to join Matrix room");
            server_error("failed to join Matrix room")
        })?;

    Ok(Json(RoomIdResponse {
        room_id: room.room_id().to_string(),
    }))
}

async fn leave_room(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<LeaveRoomApiRequest>,
) -> ApiResult<Json<LeaveRoomResponse>> {
    authorize_request(&state.api_secret_token, &headers)?;
    let room = resolve_known_room(&state.client, &request.room_id).await?;
    let room_id = room.room_id().to_owned();
    let reason = normalize_optional_reason(request.reason);

    info!(room_id = %room_id, reason = ?reason, "leaving Matrix room");
    let mut matrix_request = LeaveRoomRequest::new(room_id.clone());
    matrix_request.reason = reason;

    state.client.send(matrix_request).await.map_err(|error| {
        error!(%error, room_id = %room_id, "failed to leave Matrix room");
        server_error("failed to leave Matrix room")
    })?;

    Ok(Json(LeaveRoomResponse { success: true }))
}

async fn create_room(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateRoomApiRequest>,
) -> ApiResult<Json<RoomIdResponse>> {
    authorize_request(&state.api_secret_token, &headers)?;
    let matrix_request = build_create_room_request(request)?;

    info!(
        name = ?matrix_request.name,
        visibility = ?matrix_request.visibility,
        encrypted = !matrix_request.initial_state.is_empty(),
        "creating Matrix room"
    );
    let room = state
        .client
        .create_room(matrix_request)
        .await
        .map_err(|error| {
            error!(%error, "failed to create Matrix room");
            server_error("failed to create Matrix room")
        })?;

    Ok(Json(RoomIdResponse {
        room_id: room.room_id().to_string(),
    }))
}

async fn invite_room(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<InviteRoomApiRequest>,
) -> ApiResult<Json<RoomIdResponse>> {
    authorize_request(&state.api_secret_token, &headers)?;
    let room = resolve_known_room(&state.client, &request.room_id).await?;
    let room_id = room.room_id().to_owned();
    let matrix_requests =
        build_invite_room_requests(room_id.clone(), request.user_ids, request.reason)?;

    info!(
        room_id = %room_id,
        invite_count = matrix_requests.len(),
        "inviting Matrix users to room"
    );
    for matrix_request in matrix_requests {
        state.client.send(matrix_request).await.map_err(|error| {
            error!(%error, room_id = %room_id, "failed to invite Matrix user to room");
            server_error("failed to invite Matrix user to room")
        })?;
    }

    Ok(Json(RoomIdResponse {
        room_id: room_id.to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_or_alias_parser_accepts_room_ids_and_aliases() {
        assert!(parse_room_or_alias_id("").is_err());
        assert!(parse_room_or_alias_id("invalid").is_err());
        assert_eq!(
            parse_room_or_alias_id(" !room:example.org ")
                .unwrap()
                .as_str(),
            "!room:example.org"
        );
        assert_eq!(
            parse_room_or_alias_id(" #alias:example.org ")
                .unwrap()
                .as_str(),
            "#alias:example.org"
        );
    }

    #[test]
    fn event_id_parser_rejects_empty_and_invalid_ids() {
        assert!(parse_event_id("").is_err());
        assert!(parse_event_id("not-an-event-id").is_err());
        assert_eq!(
            parse_event_id("$event:example.org").unwrap().as_str(),
            "$event:example.org"
        );
    }

    #[test]
    fn normalize_optional_reason_trims_and_omits_blank_values() {
        assert_eq!(normalize_optional_reason(None), None);
        assert_eq!(normalize_optional_reason(Some("   ".to_owned())), None);
        assert_eq!(
            normalize_optional_reason(Some(" Workflow completed ".to_owned())),
            Some("Workflow completed".to_owned())
        );
    }

    #[test]
    fn build_create_room_request_populates_encrypted_private_room() {
        let req = build_create_room_request(CreateRoomApiRequest {
            name: " Incident #402 ".to_owned(),
            visibility: "private".to_owned(),
            topic: Some(" War room ".to_owned()),
            room_alias_name: Some(" incident-402 ".to_owned()),
            invite: Some(vec![
                " @bob:example.org ".to_owned(),
                "@alice:example.org".to_owned(),
                "@bob:example.org".to_owned(),
            ]),
            is_encrypted: true,
        })
        .unwrap();

        assert_eq!(req.name.as_deref(), Some("Incident #402"));
        assert_eq!(req.visibility, Visibility::Private);
        assert_eq!(req.preset, Some(RoomPreset::PrivateChat));
        assert_eq!(req.topic.as_deref(), Some("War room"));
        assert_eq!(req.room_alias_name.as_deref(), Some("incident-402"));
        assert_eq!(
            req.invite
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<&str>>(),
            vec!["@alice:example.org", "@bob:example.org"]
        );
        assert_eq!(req.initial_state.len(), 1);
    }

    #[test]
    fn build_create_room_request_supports_unencrypted_public_room() {
        let req = build_create_room_request(CreateRoomApiRequest {
            name: "General".to_owned(),
            visibility: "public".to_owned(),
            topic: Some("   ".to_owned()),
            room_alias_name: None,
            invite: None,
            is_encrypted: false,
        })
        .unwrap();

        assert_eq!(req.name.as_deref(), Some("General"));
        assert_eq!(req.visibility, Visibility::Public);
        assert_eq!(req.preset, Some(RoomPreset::PublicChat));
        assert_eq!(req.topic, None);
        assert_eq!(req.room_alias_name, None);
        assert!(req.invite.is_empty());
        assert!(req.initial_state.is_empty());
    }

    #[test]
    fn build_create_room_request_rejects_invalid_inputs() {
        assert!(
            build_create_room_request(CreateRoomApiRequest {
                name: "   ".to_owned(),
                visibility: "private".to_owned(),
                topic: None,
                room_alias_name: None,
                invite: None,
                is_encrypted: true,
            })
            .is_err()
        );

        assert!(
            build_create_room_request(CreateRoomApiRequest {
                name: "Room".to_owned(),
                visibility: "invalid".to_owned(),
                topic: None,
                room_alias_name: None,
                invite: None,
                is_encrypted: true,
            })
            .is_err()
        );

        assert!(
            build_create_room_request(CreateRoomApiRequest {
                name: "Room".to_owned(),
                visibility: "private".to_owned(),
                topic: None,
                room_alias_name: Some("#bad:example.org".to_owned()),
                invite: None,
                is_encrypted: true,
            })
            .is_err()
        );

        assert!(
            build_create_room_request(CreateRoomApiRequest {
                name: "Room".to_owned(),
                visibility: "private".to_owned(),
                topic: None,
                room_alias_name: None,
                invite: Some(vec!["not-a-user".to_owned()]),
                is_encrypted: true,
            })
            .is_err()
        );
    }

    #[test]
    fn room_responses_serialize_expected_fields() {
        assert_eq!(
            serde_json::to_value(RoomIdResponse {
                room_id: "!room:example.org".to_owned()
            })
            .unwrap(),
            serde_json::json!({"room_id": "!room:example.org"})
        );
        assert_eq!(
            serde_json::to_value(LeaveRoomResponse { success: true }).unwrap(),
            serde_json::json!({"success": true})
        );
    }

    #[test]
    fn build_invite_room_requests_deduplicates_sorts_and_normalizes_reason() {
        let room_id: OwnedRoomId = "!room:example.org".parse().unwrap();
        let requests = build_invite_room_requests(
            room_id.clone(),
            vec![
                " @bob:example.org ".to_owned(),
                "@alice:example.org".to_owned(),
                "@bob:example.org".to_owned(),
            ],
            Some(" Join incident room ".to_owned()),
        )
        .unwrap();

        assert_eq!(requests.len(), 2);
        let recipients: Vec<(&str, Option<&str>)> = requests
            .iter()
            .map(|req| match &req.recipient {
                InvitationRecipient::UserId(recipient) => {
                    (recipient.user_id.as_str(), recipient.reason.as_deref())
                }
                _ => panic!("expected UserId recipient"),
            })
            .collect();
        assert_eq!(
            recipients,
            vec![
                ("@alice:example.org", Some("Join incident room")),
                ("@bob:example.org", Some("Join incident room")),
            ]
        );
        assert!(requests.iter().all(|req| req.room_id == room_id));
    }

    #[test]
    fn build_invite_room_requests_omits_blank_reason_and_rejects_invalid_user_ids() {
        let room_id: OwnedRoomId = "!room:example.org".parse().unwrap();
        let requests = build_invite_room_requests(
            room_id.clone(),
            vec!["@alice:example.org".to_owned()],
            Some("   ".to_owned()),
        )
        .unwrap();
        assert_eq!(requests.len(), 1);
        match &requests[0].recipient {
            InvitationRecipient::UserId(recipient) => {
                assert_eq!(recipient.user_id.as_str(), "@alice:example.org");
                assert_eq!(recipient.reason, None);
            }
            _ => panic!("expected UserId recipient"),
        }

        assert!(build_invite_room_requests(room_id.clone(), vec![], None).is_err());
        assert!(
            build_invite_room_requests(room_id.clone(), vec!["   ".to_owned()], None).is_err()
        );
        assert!(
            build_invite_room_requests(
                room_id,
                vec!["@alice:example.org".to_owned(), "invalid-user".to_owned()],
                None,
            )
            .is_err()
        );
    }
}
