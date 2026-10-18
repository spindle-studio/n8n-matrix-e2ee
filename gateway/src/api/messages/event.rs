use matrix_sdk::ruma::events::room::message::{
    ImageMessageEventContent, MessageFormat, MessageType, RoomMessageEventContent,
};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GatewayMessageType {
    Text,
    Image,
    Audio,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct GatewayMentions {
    user_ids: Vec<String>,
    room: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event_type")]
pub(crate) enum GatewayEvent {
    #[serde(rename = "room.message")]
    Message {
        room_id: String,
        sender: String,
        body: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        formatted_body: Option<String>,
        event_id: String,
        message_type: GatewayMessageType,
        #[serde(skip_serializing_if = "Option::is_none")]
        mentions: Option<GatewayMentions>,
        #[serde(skip_serializing_if = "Option::is_none")]
        file_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        size_bytes: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        width: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        height: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_seconds: Option<u64>,
    },
}

pub(crate) fn gateway_message_type_label(message_type: &MessageType) -> &'static str {
    match message_type {
        MessageType::Text(_) => "text",
        MessageType::Image(_) => "image",
        _ => "other",
    }
}

pub(crate) fn room_message_content_to_gateway_event(
    room_id: &str,
    sender: &str,
    event_id: &str,
    message_event: &RoomMessageEventContent,
) -> Option<GatewayEvent> {
    let mentions = message_event
        .mentions
        .as_ref()
        .map(|mentions| GatewayMentions {
            user_ids: mentions.user_ids.iter().map(ToString::to_string).collect(),
            room: mentions.room,
        });
    match &message_event.msgtype {
        MessageType::Text(content) => Some(GatewayEvent::Message {
            room_id: room_id.to_owned(),
            sender: sender.to_owned(),
            body: content.body.clone(),
            formatted_body: content
                .formatted
                .as_ref()
                .filter(|formatted| formatted.format == MessageFormat::Html)
                .map(|formatted| formatted.body.clone()),
            event_id: event_id.to_owned(),
            message_type: GatewayMessageType::Text,
            mentions,
            file_name: None,
            mime_type: None,
            size_bytes: None,
            width: None,
            height: None,
            duration_seconds: None,
        }),
        MessageType::Image(content) => {
            let (width, height, size_bytes) = image_dimensions_and_size(content);
            Some(GatewayEvent::Message {
                room_id: room_id.to_owned(),
                sender: sender.to_owned(),
                body: content.body.clone(),
                formatted_body: None,
                event_id: event_id.to_owned(),
                message_type: GatewayMessageType::Image,
                mentions,
                file_name: Some(content.filename().to_owned()),
                mime_type: content.info.as_ref().and_then(|info| info.mimetype.clone()),
                size_bytes,
                width,
                height,
                duration_seconds: None,
            })
        }
        MessageType::Audio(content) => Some(GatewayEvent::Message {
            room_id: room_id.to_owned(),
            sender: sender.to_owned(),
            body: content.body.clone(),
            formatted_body: None,
            event_id: event_id.to_owned(),
            message_type: GatewayMessageType::Audio,
            mentions,
            file_name: Some(content.filename().to_owned()),
            mime_type: content.info.as_ref().and_then(|info| info.mimetype.clone()),
            size_bytes: content
                .info
                .as_ref()
                .and_then(|info| info.size)
                .map(u64::from),
            width: None,
            height: None,
            duration_seconds: content
                .info
                .as_ref()
                .and_then(|info| info.duration)
                .map(|duration| duration.as_secs()),
        }),
        _ => None,
    }
}

fn image_dimensions_and_size(
    content: &ImageMessageEventContent,
) -> (Option<u64>, Option<u64>, Option<u64>) {
    let dimensions = content.info.as_ref();
    (
        dimensions
            .and_then(|info| info.width)
            .map(|value| i64::from(value) as u64),
        dimensions
            .and_then(|info| info.height)
            .map(|value| i64::from(value) as u64),
        dimensions
            .and_then(|info| info.size)
            .map(|value| i64::from(value) as u64),
    )
}

#[cfg(test)]
mod tests {
    use ruma::{
        events::room::{
            MediaSource,
            message::{AudioInfo, AudioMessageEventContent},
        },
        owned_mxc_uri,
    };

    use super::*;

    #[test]
    fn text_gateway_events_omit_media_fields() {
        let message = GatewayEvent::Message {
            room_id: "!room:example.org".to_owned(),
            sender: "@sender:example.org".to_owned(),
            body: "Hello".to_owned(),
            formatted_body: None,
            event_id: "$message".to_owned(),
            message_type: GatewayMessageType::Text,
            mentions: None,
            file_name: None,
            mime_type: None,
            size_bytes: None,
            width: None,
            height: None,
            duration_seconds: None,
        };

        assert_eq!(
            serde_json::to_value(message).unwrap(),
            serde_json::json!({
                "event_type": "room.message",
                "room_id": "!room:example.org",
                "sender": "@sender:example.org",
                "body": "Hello",
                "event_id": "$message",
                "message_type": "text"
            })
        );
    }

    #[test]
    fn text_gateway_events_include_html_formatted_body_only_for_text() {
        let message = RoomMessageEventContent::text_html("Hello world", "<p>Hello <b>world</b></p>");
        let event = room_message_content_to_gateway_event(
            "!room:example.org",
            "@sender:example.org",
            "$html",
            &message,
        )
        .unwrap();

        assert_eq!(
            serde_json::to_value(event).unwrap(),
            serde_json::json!({
                "event_type": "room.message",
                "room_id": "!room:example.org",
                "sender": "@sender:example.org",
                "body": "Hello world",
                "formatted_body": "<p>Hello <b>world</b></p>",
                "event_id": "$html",
                "message_type": "text"
            })
        );
    }

    #[test]
    fn image_gateway_events_include_media_metadata() {
        let message = GatewayEvent::Message {
            room_id: "!room:example.org".to_owned(),
            sender: "@sender:example.org".to_owned(),
            body: "Cat".to_owned(),
            formatted_body: None,
            event_id: "$image".to_owned(),
            message_type: GatewayMessageType::Image,
            mentions: None,
            file_name: Some("cat.png".to_owned()),
            mime_type: Some("image/png".to_owned()),
            size_bytes: Some(42),
            width: Some(640),
            height: Some(480),
            duration_seconds: None,
        };
        assert_eq!(
            serde_json::to_value(message).unwrap(),
            serde_json::json!({
                "event_type": "room.message", "room_id": "!room:example.org", "sender": "@sender:example.org", "body": "Cat", "event_id": "$image",
                "message_type": "image", "file_name": "cat.png", "mime_type": "image/png", "size_bytes": 42, "width": 640, "height": 480
            })
        );
    }

    #[test]
    fn audio_gateway_events_include_stable_metadata() {
        let mut content = AudioMessageEventContent::new(
            "Audio message".to_owned(),
            MediaSource::Plain(owned_mxc_uri!("mxc://example.org/audio")),
        );
        content.filename = Some("audio.ogg".to_owned());
        let mut info = AudioInfo::new();
        info.duration = Some(std::time::Duration::from_secs(3));
        info.mimetype = Some("audio/ogg".to_owned());
        info.size = ruma::UInt::new(42);
        content.info = Some(Box::new(info));
        let message = RoomMessageEventContent::new(MessageType::Audio(content));
        let event = room_message_content_to_gateway_event(
            "!room:example.org",
            "@sender:example.org",
            "$audio",
            &message,
        )
        .unwrap();

        assert_eq!(
            serde_json::to_value(event).unwrap(),
            serde_json::json!({
                "event_type": "room.message",
                "room_id": "!room:example.org",
                "sender": "@sender:example.org",
                "body": "Audio message",
                "event_id": "$audio",
                "message_type": "audio",
                "file_name": "audio.ogg",
                "mime_type": "audio/ogg",
                "size_bytes": 42,
                "duration_seconds": 3,
            })
        );
    }

    #[test]
    fn image_and_audio_events_never_forward_formatted_body() {
        for msgtype in ["m.image", "m.audio"] {
            let content: RoomMessageEventContent = serde_json::from_value(serde_json::json!({
                "msgtype": msgtype,
                "body": "Caption",
                "format": "org.matrix.custom.html",
                "formatted_body": "<b>Caption</b>",
                "url": "mxc://example.org/media",
            }))
            .unwrap();
            let event = room_message_content_to_gateway_event(
                "!room:example.org",
                "@sender:example.org",
                "$event",
                &content,
            )
            .unwrap();
            let payload = serde_json::to_value(event).unwrap();
            assert!(payload.get("formatted_body").is_none(), "{msgtype}");
            assert!(payload.get("caption").is_none(), "{msgtype}");
        }
    }
    #[test]
    fn forwards_structured_mentions_for_all_supported_message_types() {
        for msgtype in ["m.text", "m.image", "m.audio"] {
            for (metadata, expected) in [
                (
                    serde_json::json!({}),
                    serde_json::json!({"user_ids": [], "room": false}),
                ),
                (
                    serde_json::json!({"room": true}),
                    serde_json::json!({"user_ids": [], "room": true}),
                ),
                (
                    serde_json::json!({"user_ids": ["@ben:example.org"]}),
                    serde_json::json!({"user_ids": ["@ben:example.org"], "room": false}),
                ),
                (
                    serde_json::json!({"user_ids": ["@ben:example.org", "@alice:example.org", "@ben:example.org"], "room": true}),
                    serde_json::json!({"user_ids": ["@alice:example.org", "@ben:example.org"], "room": true}),
                ),
            ] {
                let content: RoomMessageEventContent = serde_json::from_value(serde_json::json!({
                    "msgtype": msgtype,
                    "body": "Hello",
                    "url": "mxc://example.org/media",
                    "m.mentions": metadata,
                }))
                .unwrap();
                let event = room_message_content_to_gateway_event(
                    "!room:example.org",
                    "@sender:example.org",
                    "$event",
                    &content,
                )
                .unwrap();
                let payload = serde_json::to_value(event).unwrap();
                assert_eq!(payload["mentions"], expected, "{msgtype}");
                assert_eq!(payload["body"], "Hello");
            }
        }
    }

    #[test]
    fn does_not_infer_mentions_from_body_or_formatted_text() {
        for msgtype in ["m.text", "m.image", "m.audio"] {
            let content: RoomMessageEventContent = serde_json::from_value(serde_json::json!({
                "msgtype": msgtype,
                "body": "@room @ben:example.org",
                "format": "org.matrix.custom.html",
                "formatted_body": "<a href=\"https://matrix.to/#/@ben:example.org\">Ben</a>",
                "url": "mxc://example.org/media",
            }))
            .unwrap();
            let event = room_message_content_to_gateway_event(
                "!room:example.org",
                "@sender:example.org",
                "$event",
                &content,
            )
            .unwrap();
            assert!(
                serde_json::to_value(event)
                    .unwrap()
                    .get("mentions")
                    .is_none()
            );
        }
    }
}
