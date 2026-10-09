use axum::{
    Json,
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, header},
};
use matrix_sdk::{
    attachment::{AttachmentConfig, AttachmentInfo, BaseImageInfo},
    ruma::{UInt, events::room::message::TextMessageEventContent},
};
use mime::Mime;
use serde::Deserialize;
use tracing::{error, info};

use crate::{
    api::{
        auth::authorize_request,
        error::{ApiResult, client_error, server_error},
        room::{get_known_room, parse_room_id},
    },
    gateway::AppState,
};

use super::{MessageActionResponse, event::GatewayMessageType, message_action_response};

#[derive(Debug, Deserialize)]
pub(super) struct SendImageQuery {
    room_id: String,
    file_name: Option<String>,
    caption: Option<String>,
}

pub(super) async fn send_image(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SendImageQuery>,
    body: Bytes,
) -> ApiResult<Json<MessageActionResponse>> {
    authorize_request(&state.api_secret_token, &headers)?;
    if body.is_empty() {
        return Err(client_error("image body must not be empty"));
    }

    let content_type = request_content_type(&headers)?;
    if content_type.type_() != mime::IMAGE {
        return Err(client_error("Content-Type must be an image MIME type"));
    }

    let room_id = parse_room_id(&query.room_id)?;
    let room = get_known_room(&state.client, &room_id)?;
    let file_name = resolved_file_name(query.file_name.as_deref(), &content_type);
    let caption = trimmed_non_empty(query.caption.as_deref());

    let mut attachment_config =
        AttachmentConfig::new().info(AttachmentInfo::Image(BaseImageInfo {
            size: UInt::new(body.len() as u64),
            ..BaseImageInfo::default()
        }));
    if let Some(caption) = caption {
        attachment_config =
            attachment_config.caption(Some(TextMessageEventContent::plain(caption.to_owned())));
    }

    info!(room_id = %room_id, file_name, mime_type = %content_type, message_type = "image", "sending Matrix image message");
    let response = room.send_attachment(file_name.clone(), &content_type, body.to_vec(), attachment_config).await.map_err(|error| {
        error!(%error, room_id = %room_id, file_name, mime_type = %content_type, "failed to send Matrix image message");
        server_error("failed to send Matrix image message")
    })?;

    Ok(message_action_response(
        room_id.to_string(),
        response.event_id.to_string(),
        GatewayMessageType::Image,
    ))
}

pub(crate) fn request_content_type(headers: &HeaderMap) -> ApiResult<Mime> {
    let value = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| client_error("Content-Type header is required"))?;
    value
        .parse()
        .map_err(|_| client_error("Content-Type header must be a valid MIME type"))
}

pub(crate) fn resolved_file_name(file_name: Option<&str>, content_type: &Mime) -> String {
    if let Some(file_name) = trimmed_non_empty(file_name) {
        return file_name.to_owned();
    }
    let subtype = content_type
        .subtype()
        .as_str()
        .split('+')
        .next()
        .unwrap_or("image");
    format!("image.{subtype}")
}

fn trimmed_non_empty(value: Option<&str>) -> Option<&str> {
    value.and_then(|value| (!value.trim().is_empty()).then_some(value.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_name_falls_back_to_content_type() {
        let content_type: Mime = "image/png".parse().unwrap();
        assert_eq!(resolved_file_name(None, &content_type), "image.png");
    }
}
