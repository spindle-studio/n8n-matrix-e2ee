use matrix_sdk::ruma::events::{
    reaction::ReactionEventContent,
    room::{
        member::{MembershipState, OriginalSyncRoomMemberEvent, StrippedRoomMemberEvent},
        message::{
            ImageMessageEventContent, MessageFormat, MessageType, Relation,
            RoomMessageEventContent,
        },
    },
};
use serde::{Deserialize, Serialize};

use super::RelatesTo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum GatewayRelationType {
    Thread,
    Reply,
    Edit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum GatewayMessageType {
    Text,
    Notice,
    Emote,
    Image,
    Audio,
}

impl GatewayMessageType {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Notice => "notice",
            Self::Emote => "emote",
            Self::Image => "image",
            Self::Audio => "audio",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GatewayMentions {
    user_ids: Vec<String>,
    room: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "eventType", rename_all = "camelCase")]
pub(crate) enum GatewayEvent {
    #[serde(rename_all = "camelCase")]
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
        relation_type: Option<GatewayRelationType>,
        #[serde(skip_serializing_if = "Option::is_none")]
        target_event_id: Option<String>,
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
    #[serde(rename_all = "camelCase")]
    Reaction {
        room_id: String,
        sender: String,
        event_id: String,
        target_event_id: String,
        key: String,
    },
    #[serde(rename_all = "camelCase")]
    Invite {
        room_id: String,
        sender: String,
        user_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Join {
        room_id: String,
        sender: String,
        user_id: String,
        event_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Leave {
        room_id: String,
        sender: String,
        user_id: String,
        event_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

fn normalized_reason(reason: Option<&str>) -> Option<String> {
    reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) fn stripped_room_invite_to_gateway_event(
    room_id: &str,
    event: &StrippedRoomMemberEvent,
) -> Option<GatewayEvent> {
    if event.content.membership != MembershipState::Invite {
        return None;
    }

    Some(GatewayEvent::Invite {
        room_id: room_id.to_owned(),
        sender: event.sender.to_string(),
        user_id: event.state_key.to_string(),
        reason: normalized_reason(event.content.reason.as_deref()),
    })
}

pub(crate) fn room_member_event_to_gateway_event(
    room_id: &str,
    event: &OriginalSyncRoomMemberEvent,
) -> Option<GatewayEvent> {
    let prev_membership = event
        .unsigned
        .prev_content
        .as_ref()
        .map(|prev| &prev.membership);

    match &event.content.membership {
        MembershipState::Join => {
            if matches!(prev_membership, Some(MembershipState::Join)) {
                return None;
            }
            Some(GatewayEvent::Join {
                room_id: room_id.to_owned(),
                sender: event.sender.to_string(),
                user_id: event.state_key.to_string(),
                event_id: event.event_id.to_string(),
            })
        }
        MembershipState::Leave | MembershipState::Ban => {
            if matches!(
                prev_membership,
                Some(MembershipState::Leave | MembershipState::Ban)
            ) {
                return None;
            }
            Some(GatewayEvent::Leave {
                room_id: room_id.to_owned(),
                sender: event.sender.to_string(),
                user_id: event.state_key.to_string(),
                event_id: event.event_id.to_string(),
                reason: normalized_reason(event.content.reason.as_deref()),
            })
        }
        _ => None,
    }
}

pub(crate) fn reaction_content_to_gateway_event(
    room_id: &str,
    sender: &str,
    event_id: &str,
    reaction_event: &ReactionEventContent,
) -> Option<GatewayEvent> {
    let key = reaction_event.relates_to.key.trim();
    if key.is_empty() {
        return None;
    }

    let target_event_id = reaction_event.relates_to.event_id.to_string();
    Some(GatewayEvent::Reaction {
        room_id: room_id.to_owned(),
        sender: sender.to_owned(),
        event_id: event_id.to_owned(),
        target_event_id,
        key: key.to_owned(),
    })
}

pub(crate) fn gateway_message_type_label(message_type: &MessageType) -> &'static str {
    match message_type {
        MessageType::Text(_) => "text",
        MessageType::Notice(_) => "notice",
        MessageType::Emote(_) => "emote",
        MessageType::Image(_) => "image",
        MessageType::Audio(_) => "audio",
        _ => "other",
    }
}

pub(crate) fn room_message_content_to_gateway_event(
    room_id: &str,
    sender: &str,
    event_id: &str,
    message_event: &RoomMessageEventContent,
) -> Option<GatewayEvent> {
    let (msgtype, mentions_source, relates_to) = match &message_event.relates_to {
        Some(Relation::Thread(thread)) => (
            &message_event.msgtype,
            message_event.mentions.as_ref(),
            Some(RelatesTo::Thread {
                event_id: thread.event_id.to_string(),
            }),
        ),
        Some(Relation::Reply(reply)) => (
            &message_event.msgtype,
            message_event.mentions.as_ref(),
            Some(RelatesTo::Reply {
                event_id: reply.in_reply_to.event_id.to_string(),
            }),
        ),
        Some(Relation::Replacement(replacement)) => (
            &replacement.new_content.msgtype,
            replacement.new_content.mentions.as_ref(),
            Some(RelatesTo::Edit {
                event_id: replacement.event_id.to_string(),
            }),
        ),
        _ => (&message_event.msgtype, message_event.mentions.as_ref(), None),
    };

    let mentions = mentions_source.map(|mentions| GatewayMentions {
        user_ids: mentions.user_ids.iter().map(ToString::to_string).collect(),
        room: mentions.room,
    });
    let relation_type = relates_to.as_ref().map(RelatesTo::relation_type);
    let target_event_id = relates_to
        .as_ref()
        .map(|rel| rel.target_event_id().to_owned());
    match msgtype {
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
            relation_type,
            target_event_id,
            file_name: None,
            mime_type: None,
            size_bytes: None,
            width: None,
            height: None,
            duration_seconds: None,
        }),
        MessageType::Notice(content) => Some(GatewayEvent::Message {
            room_id: room_id.to_owned(),
            sender: sender.to_owned(),
            body: content.body.clone(),
            formatted_body: content
                .formatted
                .as_ref()
                .filter(|formatted| formatted.format == MessageFormat::Html)
                .map(|formatted| formatted.body.clone()),
            event_id: event_id.to_owned(),
            message_type: GatewayMessageType::Notice,
            mentions,
            relation_type,
            target_event_id,
            file_name: None,
            mime_type: None,
            size_bytes: None,
            width: None,
            height: None,
            duration_seconds: None,
        }),
        MessageType::Emote(content) => Some(GatewayEvent::Message {
            room_id: room_id.to_owned(),
            sender: sender.to_owned(),
            body: content.body.clone(),
            formatted_body: content
                .formatted
                .as_ref()
                .filter(|formatted| formatted.format == MessageFormat::Html)
                .map(|formatted| formatted.body.clone()),
            event_id: event_id.to_owned(),
            message_type: GatewayMessageType::Emote,
            mentions,
            relation_type,
            target_event_id,
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
                relation_type,
                target_event_id,
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
            relation_type,
            target_event_id,
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
            relation_type: None,
            target_event_id: None,
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
                "eventType": "message",
                "roomId": "!room:example.org",
                "sender": "@sender:example.org",
                "body": "Hello",
                "eventId": "$message",
                "messageType": "text"
            })
        );
    }

    #[test]
    fn text_notice_and_emote_gateway_events_include_html_formatted_body() {
        for (message, expected_type) in [
            (
                RoomMessageEventContent::text_html("Hello world", "<p>Hello <b>world</b></p>"),
                "text",
            ),
            (
                RoomMessageEventContent::notice_html("Hello world", "<p>Hello <b>world</b></p>"),
                "notice",
            ),
            (
                RoomMessageEventContent::emote_html("Hello world", "<p>Hello <b>world</b></p>"),
                "emote",
            ),
        ] {
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
                    "eventType": "message",
                    "roomId": "!room:example.org",
                    "sender": "@sender:example.org",
                    "body": "Hello world",
                    "formattedBody": "<p>Hello <b>world</b></p>",
                    "eventId": "$html",
                    "messageType": expected_type
                })
            );
        }
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
            relation_type: None,
            target_event_id: None,
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
                "eventType": "message", "roomId": "!room:example.org", "sender": "@sender:example.org", "body": "Cat", "eventId": "$image",
                "messageType": "image", "fileName": "cat.png", "mimeType": "image/png", "sizeBytes": 42, "width": 640, "height": 480
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
                "eventType": "message",
                "roomId": "!room:example.org",
                "sender": "@sender:example.org",
                "body": "Audio message",
                "eventId": "$audio",
                "messageType": "audio",
                "fileName": "audio.ogg",
                "mimeType": "audio/ogg",
                "sizeBytes": 42,
                "durationSeconds": 3,
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
            assert!(payload.get("formattedBody").is_none(), "{msgtype}");
            assert!(payload.get("caption").is_none(), "{msgtype}");
        }
    }
    #[test]
    fn forwards_structured_mentions_for_all_supported_message_types() {
        for msgtype in ["m.text", "m.notice", "m.emote", "m.image", "m.audio"] {
            for (metadata, expected) in [
                (
                    serde_json::json!({}),
                    serde_json::json!({"userIds": [], "room": false}),
                ),
                (
                    serde_json::json!({"room": true}),
                    serde_json::json!({"userIds": [], "room": true}),
                ),
                (
                    serde_json::json!({"user_ids": ["@ben:example.org"]}),
                    serde_json::json!({"userIds": ["@ben:example.org"], "room": false}),
                ),
                (
                    serde_json::json!({"user_ids": ["@ben:example.org", "@alice:example.org", "@ben:example.org"], "room": true}),
                    serde_json::json!({"userIds": ["@alice:example.org", "@ben:example.org"], "room": true}),
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
        for msgtype in ["m.text", "m.notice", "m.emote", "m.image", "m.audio"] {
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

    #[test]
    fn forwards_thread_and_reply_relationships_for_all_supported_message_types() {
        for msgtype in ["m.text", "m.notice", "m.emote", "m.image", "m.audio"] {
            let thread_content: RoomMessageEventContent = serde_json::from_value(serde_json::json!({
                "msgtype": msgtype,
                "body": "Threaded reply",
                "url": "mxc://example.org/media",
                "m.relates_to": {
                    "rel_type": "m.thread",
                    "event_id": "$root:example.org",
                    "m.in_reply_to": {
                        "event_id": "$root:example.org"
                    }
                }
            }))
            .unwrap();
            let thread_event = room_message_content_to_gateway_event(
                "!room:example.org",
                "@sender:example.org",
                "$event",
                &thread_content,
            )
            .unwrap();
            let thread_payload = serde_json::to_value(thread_event).unwrap();
            assert_eq!(thread_payload["relationType"], "thread", "{msgtype}");
            assert_eq!(
                thread_payload["targetEventId"],
                "$root:example.org",
                "{msgtype}"
            );

            let reply_content: RoomMessageEventContent = serde_json::from_value(serde_json::json!({
                "msgtype": msgtype,
                "body": "Inline reply",
                "url": "mxc://example.org/media",
                "m.relates_to": {
                    "m.in_reply_to": {
                        "event_id": "$target:example.org"
                    }
                }
            }))
            .unwrap();
            let reply_event = room_message_content_to_gateway_event(
                "!room:example.org",
                "@sender:example.org",
                "$event",
                &reply_content,
            )
            .unwrap();
            let reply_payload = serde_json::to_value(reply_event).unwrap();
            assert_eq!(reply_payload["relationType"], "reply", "{msgtype}");
            assert_eq!(
                reply_payload["targetEventId"],
                "$target:example.org",
                "{msgtype}"
            );
        }
    }

    #[test]
    fn forwards_edit_relationship_using_clean_new_content() {
        for (msgtype, expected_type) in [
            ("m.text", "text"),
            ("m.notice", "notice"),
            ("m.emote", "emote"),
        ] {
            let edit_content: RoomMessageEventContent = serde_json::from_value(serde_json::json!({
                "msgtype": msgtype,
                "body": "* Edited message",
                "format": "org.matrix.custom.html",
                "formatted_body": "* <p>Edited <strong>message</strong></p>",
                "m.mentions": {},
                "m.new_content": {
                    "msgtype": msgtype,
                    "body": "Edited message",
                    "format": "org.matrix.custom.html",
                    "formatted_body": "<p>Edited <strong>message</strong></p>",
                    "m.mentions": {
                        "user_ids": ["@alice:example.org"],
                        "room": false
                    }
                },
                "m.relates_to": {
                    "rel_type": "m.replace",
                    "event_id": "$original:example.org"
                }
            }))
            .unwrap();
            let event = room_message_content_to_gateway_event(
                "!room:example.org",
                "@sender:example.org",
                "$edit_event",
                &edit_content,
            )
            .unwrap();
            let payload = serde_json::to_value(event).unwrap();
            assert_eq!(
                payload,
                serde_json::json!({
                    "eventType": "message",
                    "roomId": "!room:example.org",
                    "sender": "@sender:example.org",
                    "body": "Edited message",
                    "formattedBody": "<p>Edited <strong>message</strong></p>",
                    "eventId": "$edit_event",
                    "messageType": expected_type,
                    "mentions": {
                        "userIds": ["@alice:example.org"],
                        "room": false
                    },
                    "relationType": "edit",
                    "targetEventId": "$original:example.org"
                })
            );
        }
    }

    #[test]
    fn forwards_reaction_events_and_ignores_empty_keys() {
        let content: ReactionEventContent = serde_json::from_value(serde_json::json!({
            "m.relates_to": {
                "rel_type": "m.annotation",
                "event_id": "$target:example.org",
                "key": "👍"
            }
        }))
        .unwrap();
        let event = reaction_content_to_gateway_event(
            "!room:example.org",
            "@sender:example.org",
            "$reaction_event",
            &content,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(event).unwrap(),
            serde_json::json!({
                "eventType": "reaction",
                "roomId": "!room:example.org",
                "sender": "@sender:example.org",
                "eventId": "$reaction_event",
                "targetEventId": "$target:example.org",
                "key": "👍"
            })
        );

        let empty_content: ReactionEventContent = serde_json::from_value(serde_json::json!({
            "m.relates_to": {
                "rel_type": "m.annotation",
                "event_id": "$target:example.org",
                "key": "   "
            }
        }))
        .unwrap();
        assert!(
            reaction_content_to_gateway_event(
                "!room:example.org",
                "@sender:example.org",
                "$reaction_event",
                &empty_content,
            )
            .is_none()
        );
    }

    #[test]
    fn converts_room_invite_join_and_leave_events() {
        let stripped_invite: StrippedRoomMemberEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.member",
            "sender": "@inviter:example.org",
            "state_key": "@bot:example.org",
            "content": {
                "membership": "invite",
                "reason": " Join war room "
            }
        }))
        .unwrap();
        let invite_event =
            stripped_room_invite_to_gateway_event("!room:example.org", &stripped_invite).unwrap();
        assert_eq!(
            serde_json::to_value(invite_event).unwrap(),
            serde_json::json!({
                "eventType": "invite",
                "roomId": "!room:example.org",
                "sender": "@inviter:example.org",
                "userId": "@bot:example.org",
                "reason": "Join war room"
            })
        );

        let join_member: OriginalSyncRoomMemberEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.member",
            "event_id": "$join_event:example.org",
            "sender": "@alice:example.org",
            "state_key": "@alice:example.org",
            "origin_server_ts": 1,
            "content": {
                "membership": "join"
            }
        }))
        .unwrap();
        let join_event =
            room_member_event_to_gateway_event("!room:example.org", &join_member).unwrap();
        assert_eq!(
            serde_json::to_value(join_event).unwrap(),
            serde_json::json!({
                "eventType": "join",
                "roomId": "!room:example.org",
                "sender": "@alice:example.org",
                "userId": "@alice:example.org",
                "eventId": "$join_event:example.org"
            })
        );

        let profile_change: OriginalSyncRoomMemberEvent =
            serde_json::from_value(serde_json::json!({
                "type": "m.room.member",
                "event_id": "$profile_event:example.org",
                "sender": "@alice:example.org",
                "state_key": "@alice:example.org",
                "origin_server_ts": 2,
                "content": {
                    "membership": "join",
                    "displayname": "Alice"
                },
                "unsigned": {
                    "prev_content": {
                        "membership": "join"
                    }
                }
            }))
            .unwrap();
        assert!(room_member_event_to_gateway_event("!room:example.org", &profile_change).is_none());

        let leave_member: OriginalSyncRoomMemberEvent =
            serde_json::from_value(serde_json::json!({
                "type": "m.room.member",
                "event_id": "$leave_event:example.org",
                "sender": "@mod:example.org",
                "state_key": "@bob:example.org",
                "origin_server_ts": 3,
                "content": {
                    "membership": "leave",
                    "reason": "  Kicked for spam  "
                },
                "unsigned": {
                    "prev_content": {
                        "membership": "join"
                    }
                }
            }))
            .unwrap();
        let leave_event =
            room_member_event_to_gateway_event("!room:example.org", &leave_member).unwrap();
        assert_eq!(
            serde_json::to_value(leave_event).unwrap(),
            serde_json::json!({
                "eventType": "leave",
                "roomId": "!room:example.org",
                "sender": "@mod:example.org",
                "userId": "@bob:example.org",
                "eventId": "$leave_event:example.org",
                "reason": "Kicked for spam"
            })
        );
    }

    #[test]
    fn serializes_room_invite_events() {
        let event = GatewayEvent::Invite {
            room_id: "!room:example.org".to_owned(),
            sender: "@inviter:example.org".to_owned(),
            user_id: "@bot:example.org".to_owned(),
            reason: None,
        };
        assert_eq!(
            serde_json::to_value(event).unwrap(),
            serde_json::json!({
                "eventType": "invite",
                "roomId": "!room:example.org",
                "sender": "@inviter:example.org",
                "userId": "@bot:example.org"
            })
        );
    }
}
