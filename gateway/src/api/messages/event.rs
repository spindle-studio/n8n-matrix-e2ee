use matrix_sdk::ruma::events::room::message::{
    ImageMessageEventContent, MessageType, RoomMessageEventContent,
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
#[serde(tag = "event_type")]
pub(crate) enum GatewayEvent {
    #[serde(rename = "room.message")]
    Message {
        room_id: String,
        sender: String,
        body: String,
        event_id: String,
        message_type: GatewayMessageType,
        #[serde(skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
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
    match &message_event.msgtype {
        MessageType::Text(content) => Some(GatewayEvent::Message {
            room_id: room_id.to_owned(),
            sender: sender.to_owned(),
            body: content.body.clone(),
            event_id: event_id.to_owned(),
            message_type: GatewayMessageType::Text,
            caption: None,
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
                event_id: event_id.to_owned(),
                message_type: GatewayMessageType::Image,
                caption: content.caption().map(ToOwned::to_owned),
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
            event_id: event_id.to_owned(),
            message_type: GatewayMessageType::Audio,
            caption: content.caption().map(ToOwned::to_owned),
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
            event_id: "$message".to_owned(),
            message_type: GatewayMessageType::Text,
            caption: None,
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
    fn image_gateway_events_include_media_metadata() {
        let message = GatewayEvent::Message {
            room_id: "!room:example.org".to_owned(),
            sender: "@sender:example.org".to_owned(),
            body: "cat.png".to_owned(),
            event_id: "$image".to_owned(),
            message_type: GatewayMessageType::Image,
            caption: Some("Cat".to_owned()),
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
                "event_type": "room.message", "room_id": "!room:example.org", "sender": "@sender:example.org", "body": "cat.png", "event_id": "$image",
                "message_type": "image", "caption": "Cat", "file_name": "cat.png", "mime_type": "image/png", "size_bytes": 42, "width": 640, "height": 480
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
                "caption": "Audio message",
                "file_name": "audio.ogg",
                "mime_type": "audio/ogg",
                "size_bytes": 42,
                "duration_seconds": 3,
            })
        );
    }
}
