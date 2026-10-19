mod audio;
mod event;
mod image;
mod notice;
mod text;

use std::io::Cursor;

use axum::{Json, Router, body::Bytes, extract::Multipart, routing::post};
use matrix_sdk::{
    Room,
    ruma::{
        OwnedEventId, OwnedUserId,
        events::{
            AnyMessageLikeEventContent, Mentions,
            room::MediaSource,
        },
        serde::Raw,
    },
};
use mime::Mime;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::error;

use crate::{
    api::error::{ApiResult, client_error, server_error},
    gateway::AppState,
};

pub(crate) use event::{
    GatewayEvent, gateway_message_type_label, room_message_content_to_gateway_event,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "rel_type", rename_all = "snake_case")]
pub(crate) enum RelatesTo {
    /// Matrix Threading (m.thread)
    Thread { event_id: String },
    /// Inline reply (m.in_reply_to)
    Reply { event_id: String },
    /// Message replacement (m.replace)
    Edit { event_id: String },
}

impl RelatesTo {
    pub(crate) fn to_matrix_json(&self) -> serde_json::Value {
        match self {
            Self::Thread { event_id } => {
                json!({
                    "m.relates_to": {
                        "rel_type": "m.thread",
                        "event_id": event_id,
                        "m.in_reply_to": {
                            "event_id": event_id
                        }
                    }
                })
            }
            Self::Reply { event_id } => {
                json!({
                    "m.relates_to": {
                        "m.in_reply_to": {
                            "event_id": event_id
                        }
                    }
                })
            }
            Self::Edit { event_id } => {
                json!({
                    "m.relates_to": {
                        "rel_type": "m.replace",
                        "event_id": event_id
                    }
                })
            }
        }
    }

    fn validated(self) -> ApiResult<Self> {
        match self {
            Self::Thread { event_id } => Ok(Self::Thread {
                event_id: parse_relates_to_event_id(&event_id)?.to_string(),
            }),
            Self::Reply { event_id } => Ok(Self::Reply {
                event_id: parse_relates_to_event_id(&event_id)?.to_string(),
            }),
            Self::Edit { event_id } => Ok(Self::Edit {
                event_id: parse_relates_to_event_id(&event_id)?.to_string(),
            }),
        }
    }
}

pub(super) fn resolve_relates_to(request: Option<RelatesTo>) -> ApiResult<Option<RelatesTo>> {
    request.map(RelatesTo::validated).transpose()
}

pub(super) fn resolve_media_relates_to(request: Option<RelatesTo>) -> ApiResult<Option<RelatesTo>> {
    let relates_to = resolve_relates_to(request)?;
    if matches!(relates_to, Some(RelatesTo::Edit { .. })) {
        return Err(client_error(
            "edit relationship is only supported for text and notice messages",
        ));
    }
    Ok(relates_to)
}

fn parse_relates_to_event_id(event_id: &str) -> ApiResult<OwnedEventId> {
    let trimmed = event_id.trim();
    if trimmed.is_empty() {
        return Err(client_error("relates_to.event_id must not be empty"));
    }

    trimmed
        .parse()
        .map_err(|_| client_error("relates_to.event_id must be a valid Matrix event ID"))
}

pub(super) fn inject_relates_to_value(
    mut content: serde_json::Value,
    relates_to: Option<&RelatesTo>,
) -> serde_json::Value {
    if let Some(relates_to) = relates_to
        && let Some(obj) = content.as_object_mut()
        && let Some(rel_obj) = relates_to.to_matrix_json().get("m.relates_to")
    {
        obj.insert("m.relates_to".to_owned(), rel_obj.clone());
    }
    content
}

pub(super) fn into_raw_room_message_content(
    content: &serde_json::Value,
) -> ApiResult<Raw<AnyMessageLikeEventContent>> {
    serde_json::from_value(content.clone())
        .map_err(|_| server_error("failed to serialize Matrix message content"))
}

pub(super) async fn upload_room_media(
    client: &matrix_sdk::Client,
    room: &Room,
    content_type: &Mime,
    data: Vec<u8>,
    upload_error_message: &'static str,
) -> ApiResult<MediaSource> {
    if room
        .latest_encryption_state()
        .await
        .map_err(|error| {
            error!(%error, room_id = %room.room_id(), "failed to inspect Matrix room encryption state");
            server_error("failed to inspect Matrix room encryption state")
        })?
        .is_encrypted()
    {
        let mut reader = Cursor::new(data);
        let encrypted = client.upload_encrypted_file(&mut reader).await.map_err(|error| {
            error!(%error, room_id = %room.room_id(), "failed to upload encrypted Matrix media");
            server_error(upload_error_message)
        })?;
        Ok(MediaSource::Encrypted(Box::new(encrypted)))
    } else {
        let upload = client.media().upload(content_type, data, None).await.map_err(|error| {
            error!(%error, room_id = %room.room_id(), "failed to upload Matrix media");
            server_error(upload_error_message)
        })?;
        Ok(MediaSource::Plain(upload.content_uri))
    }
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct MentionsRequest {
    #[serde(default)]
    pub(super) user_ids: Option<Vec<String>>,
    #[serde(default)]
    pub(super) room: Option<bool>,
}

impl MentionsRequest {
    pub(super) fn into_matrix_mentions(self) -> ApiResult<Mentions> {
        let mut mentions = if self.room.unwrap_or(false) {
            Mentions::with_room_mention()
        } else {
            Mentions::new()
        };

        if let Some(raw_user_ids) = self.user_ids {
            let mut user_ids = Vec::with_capacity(raw_user_ids.len());
            for raw_user_id in raw_user_ids {
                user_ids.push(parse_user_id(&raw_user_id)?);
            }
            mentions.user_ids.extend(user_ids);
        }

        Ok(mentions)
    }
}

pub(super) fn resolve_mentions(request: Option<MentionsRequest>) -> ApiResult<Mentions> {
    request.unwrap_or_default().into_matrix_mentions()
}

fn parse_user_id(user_id: &str) -> ApiResult<OwnedUserId> {
    let trimmed = user_id.trim();
    if trimmed.is_empty() {
        return Err(client_error("mentions.user_ids entries must not be empty"));
    }

    trimmed
        .parse()
        .map_err(|_| client_error("mentions.user_ids entries must be valid Matrix user IDs"))
}

#[derive(Debug, Deserialize)]
pub(super) struct SendMediaMetadata {
    pub(super) room_id: String,
    #[serde(default)]
    pub(super) file_name: Option<String>,
    #[serde(default)]
    pub(super) body: Option<String>,
    #[serde(default)]
    pub(super) mentions: Option<MentionsRequest>,
    #[serde(default)]
    pub(super) relates_to: Option<RelatesTo>,
}

#[derive(Debug)]
pub(super) struct MultipartMediaUpload {
    pub(super) metadata: SendMediaMetadata,
    pub(super) data: Bytes,
    pub(super) content_type: Option<Mime>,
    pub(super) part_file_name: Option<String>,
}

pub(super) async fn extract_multipart_media(
    mut multipart: Multipart,
) -> ApiResult<MultipartMediaUpload> {
    let mut metadata: Option<SendMediaMetadata> = None;
    let mut media_part: Option<(Bytes, Option<Mime>, Option<String>)> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| client_error("invalid multipart/form-data payload"))?
    {
        match field.name() {
            Some("metadata") => {
                if metadata.is_some() {
                    return Err(client_error("duplicate metadata multipart field"));
                }
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|_| client_error("failed to read metadata multipart field"))?;
                let parsed: SendMediaMetadata = serde_json::from_slice(&bytes)
                    .map_err(|_| client_error("metadata field must be valid JSON"))?;
                metadata = Some(parsed);
            }
            Some("media" | "file") => {
                if media_part.is_some() {
                    return Err(client_error("duplicate media multipart field"));
                }
                let part_file_name = field.file_name().map(ToOwned::to_owned);
                let content_type = match field.content_type() {
                    Some(raw) => Some(
                        raw.parse::<Mime>()
                            .map_err(|_| client_error("media part Content-Type must be a valid MIME type"))?,
                    ),
                    None => None,
                };
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|_| client_error("failed to read media multipart field"))?;
                media_part = Some((bytes, content_type, part_file_name));
            }
            Some(_) => {
                return Err(client_error("unexpected multipart field"));
            }
            None => {
                return Err(client_error("multipart field name is required"));
            }
        }
    }

    let metadata = metadata.ok_or_else(|| client_error("metadata multipart field is required"))?;
    let (data, content_type, part_file_name) =
        media_part.ok_or_else(|| client_error("media multipart field is required"))?;

    Ok(MultipartMediaUpload {
        metadata,
        data,
        content_type,
        part_file_name,
    })
}

#[derive(Debug, Serialize)]
pub(crate) struct MessageActionResponse {
    room_id: String,
    event_id: String,
    message_type: event::GatewayMessageType,
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/text", post(text::send_text))
        .route("/notice", post(notice::send_notice))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_mentions_defaults_to_empty_mentions_object() {
        let mentions = resolve_mentions(None).unwrap();
        assert_eq!(serde_json::to_value(mentions).unwrap(), serde_json::json!({}));
    }

    #[test]
    fn resolve_mentions_deduplicates_sorts_and_trims_user_ids() {
        let mentions = resolve_mentions(Some(MentionsRequest {
            user_ids: Some(vec![
                " @ben:example.org ".to_owned(),
                "@alice:example.org".to_owned(),
                "@ben:example.org".to_owned(),
            ]),
            room: Some(true),
        }))
        .unwrap();

        assert_eq!(
            serde_json::to_value(mentions).unwrap(),
            serde_json::json!({
                "user_ids": ["@alice:example.org", "@ben:example.org"],
                "room": true
            })
        );
    }

    #[test]
    fn resolve_mentions_rejects_empty_or_invalid_user_ids() {
        assert!(
            resolve_mentions(Some(MentionsRequest {
                user_ids: Some(vec!["".to_owned()]),
                room: None,
            }))
            .is_err()
        );
        assert!(
            resolve_mentions(Some(MentionsRequest {
                user_ids: Some(vec!["not-a-user-id".to_owned()]),
                room: None,
            }))
            .is_err()
        );
    }

    #[tokio::test]
    async fn extract_multipart_media_parses_metadata_and_media_parts() {
        use axum::{body::Body, extract::FromRequest, http::Request};

        let boundary = "test-boundary-123";
        let body = format!(
            "--{boundary}\r\n\
             Content-Disposition: form-data; name=\"metadata\"\r\n\
             Content-Type: application/json\r\n\r\n\
             {{\"room_id\":\"!room:example.org\",\"file_name\":\"photo.png\",\"body\":\"Caption\",\"mentions\":{{\"user_ids\":[\"@alice:example.org\"],\"room\":true}}}}\r\n\
             --{boundary}\r\n\
             Content-Disposition: form-data; name=\"media\"; filename=\"fallback.png\"\r\n\
             Content-Type: image/png\r\n\r\n\
             png-bytes\r\n\
             --{boundary}--\r\n"
        );
        let req = Request::builder()
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap();
        let multipart = Multipart::from_request(req, &()).await.unwrap();
        let upload = extract_multipart_media(multipart).await.unwrap();

        assert_eq!(upload.metadata.room_id, "!room:example.org");
        assert_eq!(upload.metadata.file_name.as_deref(), Some("photo.png"));
        assert_eq!(upload.metadata.body.as_deref(), Some("Caption"));
        assert_eq!(upload.data.as_ref(), b"png-bytes");
        assert_eq!(
            upload.content_type.as_ref().map(Mime::essence_str),
            Some("image/png")
        );
        assert_eq!(upload.part_file_name.as_deref(), Some("fallback.png"));

        let mentions = resolve_mentions(upload.metadata.mentions).unwrap();
        assert_eq!(
            serde_json::to_value(mentions).unwrap(),
            serde_json::json!({
                "user_ids": ["@alice:example.org"],
                "room": true
            })
        );
    }

    #[tokio::test]
    async fn extract_multipart_media_rejects_missing_parts() {
        use axum::{body::Body, extract::FromRequest, http::Request};

        let boundary = "test-boundary-missing";
        let body = format!(
            "--{boundary}\r\n\
             Content-Disposition: form-data; name=\"metadata\"\r\n\r\n\
             {{\"room_id\":\"!room:example.org\"}}\r\n\
             --{boundary}--\r\n"
        );
        let req = Request::builder()
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap();
        let multipart = Multipart::from_request(req, &()).await.unwrap();
        assert!(extract_multipart_media(multipart).await.is_err());
    }

    #[test]
    fn relates_to_validates_trims_and_converts_to_matrix_json() {
        let thread = resolve_relates_to(Some(RelatesTo::Thread {
            event_id: " $root:example.org ".to_owned(),
        }))
        .unwrap()
        .unwrap();
        assert_eq!(
            thread,
            RelatesTo::Thread {
                event_id: "$root:example.org".to_owned()
            }
        );
        assert_eq!(
            thread.to_matrix_json(),
            serde_json::json!({
                "m.relates_to": {
                    "rel_type": "m.thread",
                    "event_id": "$root:example.org",
                    "m.in_reply_to": {
                        "event_id": "$root:example.org"
                    }
                }
            })
        );

        let reply = resolve_relates_to(Some(RelatesTo::Reply {
            event_id: "$reply:example.org".to_owned(),
        }))
        .unwrap()
        .unwrap();
        assert_eq!(
            reply.to_matrix_json(),
            serde_json::json!({
                "m.relates_to": {
                    "m.in_reply_to": {
                        "event_id": "$reply:example.org"
                    }
                }
            })
        );

        let edit = resolve_relates_to(Some(RelatesTo::Edit {
            event_id: "$edit:example.org".to_owned(),
        }))
        .unwrap()
        .unwrap();
        assert_eq!(
            edit.to_matrix_json(),
            serde_json::json!({
                "m.relates_to": {
                    "rel_type": "m.replace",
                    "event_id": "$edit:example.org"
                }
            })
        );
    }

    #[test]
    fn relates_to_rejects_empty_invalid_and_media_edit_relationships() {
        assert!(
            resolve_relates_to(Some(RelatesTo::Thread {
                event_id: "   ".to_owned(),
            }))
            .is_err()
        );
        assert!(
            resolve_relates_to(Some(RelatesTo::Reply {
                event_id: "not-an-event-id".to_owned(),
            }))
            .is_err()
        );
        assert!(
            resolve_media_relates_to(Some(RelatesTo::Edit {
                event_id: "$edit:example.org".to_owned(),
            }))
            .is_err()
        );
        assert!(
            resolve_media_relates_to(Some(RelatesTo::Thread {
                event_id: "$root:example.org".to_owned(),
            }))
            .is_ok()
        );
    }
}

