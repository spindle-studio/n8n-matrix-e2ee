use axum::{
    Json,
    extract::{Multipart, State},
    http::HeaderMap,
};
use matrix_sdk::ruma::{
    UInt,
    events::{
        Mentions,
        room::{
            ImageInfo, MediaSource,
            message::{ImageMessageEventContent, MessageType, RoomMessageEventContent},
        },
    },
};
use mime::Mime;
use tracing::{error, info};

use crate::{
    api::{
        auth::authorize_request,
        error::{ApiResult, client_error, server_error},
        room::resolve_known_room,
    },
    gateway::{AppState, media_cache::MediaCacheKey},
};

use super::{
    MessageActionResponse, RelatesTo, event::GatewayMessageType, extract_multipart_media,
    inject_relates_to_value, into_raw_room_message_content, is_room_encrypted,
    message_action_response, resolve_media_relates_to, resolve_mentions,
    upload_room_media_with_encryption,
};

pub(super) async fn send_image(
    State(state): State<AppState>,
    headers: HeaderMap,
    multipart: Multipart,
) -> ApiResult<Json<MessageActionResponse>> {
    let account = authorize_request(&state, &headers)?;
    let upload = extract_multipart_media(multipart).await?;
    if upload.data.is_empty() {
        return Err(client_error("image body must not be empty"));
    }

    let content_type = validate_image_content_type(upload.content_type)?;
    let mentions = resolve_mentions(upload.metadata.mentions)?;
    let relates_to = resolve_media_relates_to(
        upload.metadata.relation_type,
        upload.metadata.target_event_id.as_deref(),
    )?;
    let room = resolve_known_room(&account.client, &upload.metadata.room_id).await?;
    let room_id = room.room_id().to_owned();
    let file_name = resolved_file_name(
        upload
            .metadata
            .file_name
            .as_deref()
            .or(upload.part_file_name.as_deref()),
        &content_type,
    );
    let message_body = trimmed_non_empty(upload.metadata.body.as_deref());
    let size_bytes = upload.data.len() as u64;
    let is_encrypted = is_room_encrypted(&room).await?;
    let cache_key = MediaCacheKey::new(&room_id, is_encrypted, &content_type, &upload.data);
    let source = if let Some(cached_source) = account.media_cache.get(&cache_key) {
        info!(
            account = %account.account_id,
            room_id = %room_id,
            file_name,
            mime_type = %content_type,
            size_bytes,
            sha256 = %cache_key.sha256,
            "reusing cached Matrix image upload"
        );
        cached_source
    } else {
        let uploaded_source = upload_room_media_with_encryption(
            &account.client,
            &room,
            is_encrypted,
            &content_type,
            upload.data.to_vec(),
            "failed to upload Matrix image media",
        )
        .await?;
        account.media_cache.put(&cache_key, &uploaded_source);
        uploaded_source
    };
    let payload = image_room_message_payload(
        message_body,
        file_name.clone(),
        source,
        &content_type,
        size_bytes,
        mentions,
        relates_to.as_ref(),
    )?;
    let raw_content = into_raw_room_message_content(&payload)?;

    info!(account = %account.account_id, room_id = %room_id, file_name, mime_type = %content_type, message_type = "image", "sending Matrix image message");
    let response = room.send_raw("m.room.message", raw_content).await.map_err(|error| {
        error!(account = %account.account_id, %error, room_id = %room_id, file_name, mime_type = %content_type, "failed to send Matrix image message");
        server_error("failed to send Matrix image message")
    })?;

    Ok(message_action_response(
        room_id.to_string(),
        response.response.event_id.to_string(),
        GatewayMessageType::Image,
    ))
}

fn image_room_message_payload(
    caption: Option<&str>,
    file_name: String,
    source: MediaSource,
    content_type: &Mime,
    size_bytes: u64,
    mentions: Mentions,
    relates_to: Option<&RelatesTo>,
) -> ApiResult<serde_json::Value> {
    let mut info = ImageInfo::new();
    info.mimetype = Some(content_type.to_string());
    info.size = UInt::new(size_bytes);

    let body = caption.unwrap_or(&file_name).to_owned();
    let mut content = ImageMessageEventContent::new(body, source);
    if caption.is_some() {
        content.filename = Some(file_name);
    }
    content.info = Some(Box::new(info));

    let mut message = RoomMessageEventContent::new(MessageType::Image(content));
    message.mentions = Some(mentions);

    let json = serde_json::to_value(message)
        .map_err(|_| server_error("failed to serialize Matrix image message content"))?;
    Ok(inject_relates_to_value(json, relates_to))
}

fn validate_image_content_type(content_type: Option<Mime>) -> ApiResult<Mime> {
    let content_type =
        content_type.ok_or_else(|| client_error("media part Content-Type header is required"))?;
    if content_type.type_() != mime::IMAGE {
        return Err(client_error("media part Content-Type must be an image MIME type"));
    }
    Ok(content_type)
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

    #[test]
    fn image_content_type_requires_image_mime() {
        assert!(validate_image_content_type(Some("image/png".parse().unwrap())).is_ok());
        assert!(validate_image_content_type(Some("audio/ogg".parse().unwrap())).is_err());
        assert!(validate_image_content_type(None).is_err());
    }

    #[test]
    fn image_payload_includes_relates_to_and_mentions() {
        use matrix_sdk::ruma::owned_mxc_uri;

        let content_type: Mime = "image/png".parse().unwrap();
        let thread = RelatesTo::Thread {
            event_id: "$root:example.org".to_owned(),
        };
        let payload = image_room_message_payload(
            Some("Architecture diagram"),
            "diagram.png".to_owned(),
            MediaSource::Plain(owned_mxc_uri!("mxc://example.org/image")),
            &content_type,
            128,
            Mentions::new(),
            Some(&thread),
        )
        .unwrap();

        assert_eq!(
            payload,
            serde_json::json!({
                "msgtype": "m.image",
                "body": "Architecture diagram",
                "filename": "diagram.png",
                "url": "mxc://example.org/image",
                "info": {
                    "mimetype": "image/png",
                    "size": 128
                },
                "m.mentions": {},
                "m.relates_to": {
                    "rel_type": "m.thread",
                    "event_id": "$root:example.org",
                    "m.in_reply_to": {
                        "event_id": "$root:example.org"
                    }
                }
            })
        );
    }
}
