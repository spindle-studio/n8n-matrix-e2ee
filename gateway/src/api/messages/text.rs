use axum::{Json, extract::State, http::HeaderMap};
use matrix_sdk::ruma::events::{Mentions, room::message::RoomMessageEventContent};
use pulldown_cmark::{Event, Options, Parser, TagEnd, html};
use serde::Deserialize;
use tracing::{error, info};

use crate::gateway::AppState;

use super::{
    MentionsRequest, MessageActionResponse, RelatesTo, event::GatewayMessageType,
    inject_relates_to_value, into_raw_room_message_content, message_action_response,
    resolve_mentions, resolve_relates_to,
};
use crate::api::{
    auth::authorize_request,
    error::{ApiResult, client_error, server_error},
    room::resolve_known_room,
};

#[derive(Debug, Deserialize)]
pub(super) struct SendMarkdownRequest {
    room_id: String,
    body: String,
    #[serde(default)]
    mentions: Option<MentionsRequest>,
    #[serde(default)]
    relates_to: Option<RelatesTo>,
}

pub(super) async fn send_text(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SendMarkdownRequest>,
) -> ApiResult<Json<MessageActionResponse>> {
    send_markdown_message(state, headers, request, GatewayMessageType::Text).await
}

pub(super) async fn send_markdown_message(
    state: AppState,
    headers: HeaderMap,
    request: SendMarkdownRequest,
    gateway_message_type: GatewayMessageType,
) -> ApiResult<Json<MessageActionResponse>> {
    authorize_request(&state.api_secret_token, &headers)?;
    let mentions = resolve_mentions(request.mentions)?;
    let relates_to = resolve_relates_to(request.relates_to)?;
    let payload = build_markdown_message_payload(
        &request.body,
        gateway_message_type,
        mentions,
        relates_to.as_ref(),
    )
    .ok_or_else(|| client_error("body must not be empty"))?;
    let raw_content = into_raw_room_message_content(&payload)?;

    let room = resolve_known_room(&state.client, &request.room_id).await?;
    let room_id = room.room_id().to_owned();
    info!(
        room_id = %room_id,
        message_type = gateway_message_type.as_str(),
        "sending Matrix message"
    );
    let response = room.send_raw("m.room.message", raw_content).await.map_err(|error| {
        error!(%error, room_id = %room_id, "failed to send Matrix message");
        server_error("failed to send Matrix message")
    })?;

    Ok(message_action_response(
        room_id.to_string(),
        response.response.event_id.to_string(),
        gateway_message_type,
    ))
}

#[cfg(test)]
pub(super) fn build_markdown_message_content(
    markdown: &str,
    gateway_message_type: GatewayMessageType,
    mentions: Mentions,
) -> Option<RoomMessageEventContent> {
    let (plain_body, formatted_body) = compile_markdown(markdown)?;
    build_html_message_content(plain_body, formatted_body, gateway_message_type, mentions)
}

pub(super) fn build_markdown_message_payload(
    markdown: &str,
    gateway_message_type: GatewayMessageType,
    mentions: Mentions,
    relates_to: Option<&RelatesTo>,
) -> Option<serde_json::Value> {
    let (plain_body, formatted_body) = compile_markdown(markdown)?;
    let base_content = build_html_message_content(
        plain_body.clone(),
        formatted_body.clone(),
        gateway_message_type,
        mentions.clone(),
    )?;
    let base_json = serde_json::to_value(base_content).ok()?;

    let content_json = if matches!(relates_to, Some(RelatesTo::Edit { .. })) {
        let fallback_content = build_html_message_content(
            format!("* {plain_body}"),
            format!("* {formatted_body}"),
            gateway_message_type,
            mentions,
        )?;
        let mut fallback_json = serde_json::to_value(fallback_content).ok()?;
        if let Some(obj) = fallback_json.as_object_mut() {
            obj.insert("m.new_content".to_owned(), base_json);
        }
        fallback_json
    } else {
        base_json
    };

    Some(inject_relates_to_value(content_json, relates_to))
}

fn build_html_message_content(
    plain_body: String,
    formatted_body: String,
    gateway_message_type: GatewayMessageType,
    mentions: Mentions,
) -> Option<RoomMessageEventContent> {
    let mut content = match gateway_message_type {
        GatewayMessageType::Text => Some(RoomMessageEventContent::text_html(
            plain_body,
            formatted_body,
        )),
        GatewayMessageType::Notice => Some(RoomMessageEventContent::notice_html(
            plain_body,
            formatted_body,
        )),
        GatewayMessageType::Emote => Some(RoomMessageEventContent::emote_html(
            plain_body,
            formatted_body,
        )),
        GatewayMessageType::Image | GatewayMessageType::Audio => None,
    }?;
    content.mentions = Some(mentions);
    Some(content)
}

fn compile_markdown(markdown: &str) -> Option<(String, String)> {
    if markdown.trim().is_empty() {
        return None;
    }

    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    let events: Vec<Event<'_>> = Parser::new_ext(markdown, options).collect();

    let mut plain_body = String::new();
    for event in &events {
        match event {
            Event::Text(text) | Event::Code(text) => plain_body.push_str(text),
            Event::SoftBreak | Event::HardBreak => plain_body.push('\n'),
            Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::Item
                | TagEnd::CodeBlock
                | TagEnd::BlockQuote(_)
                | TagEnd::TableRow
                | TagEnd::TableHead,
            ) => {
                if !plain_body.ends_with('\n') {
                    plain_body.push('\n');
                }
            }
            _ => {}
        }
    }

    let plain_body = plain_body.trim().to_owned();
    if plain_body.is_empty() {
        return None;
    }

    let mut formatted_body = String::new();
    html::push_html(&mut formatted_body, events.into_iter());
    let formatted_body = formatted_body.trim().to_owned();

    Some((plain_body, formatted_body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_markdown_to_plain_body_and_html_formatted_body() {
        let content = build_markdown_message_content(
            "Hello **world** and `code`!",
            GatewayMessageType::Text,
            Mentions::new(),
        )
        .unwrap();
        let json = serde_json::to_value(content).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "msgtype": "m.text",
                "body": "Hello world and code!",
                "format": "org.matrix.custom.html",
                "formatted_body": "<p>Hello <strong>world</strong> and <code>code</code>!</p>",
                "m.mentions": {}
            })
        );
    }

    #[test]
    fn converts_plain_text_to_body_and_html_formatted_body_with_mentions() {
        let mentions = resolve_mentions(Some(MentionsRequest {
            user_ids: Some(vec!["@alice:example.org".to_owned()]),
            room: Some(true),
        }))
        .unwrap();
        let content =
            build_markdown_message_content("Hello Matrix", GatewayMessageType::Text, mentions)
                .unwrap();
        let json = serde_json::to_value(content).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "msgtype": "m.text",
                "body": "Hello Matrix",
                "format": "org.matrix.custom.html",
                "formatted_body": "<p>Hello Matrix</p>",
                "m.mentions": {
                    "user_ids": ["@alice:example.org"],
                    "room": true
                }
            })
        );
    }

    #[test]
    fn rejects_empty_or_whitespace_only_body() {
        assert!(
            build_markdown_message_content("", GatewayMessageType::Text, Mentions::new()).is_none()
        );
        assert!(
            build_markdown_message_content(
                "   \n\t  ",
                GatewayMessageType::Text,
                Mentions::new()
            )
            .is_none()
        );
    }

    #[test]
    fn builds_threaded_and_reply_text_payloads() {
        let thread = RelatesTo::Thread {
            event_id: "$root:example.org".to_owned(),
        };
        let payload = build_markdown_message_payload(
            "Thread **update**",
            GatewayMessageType::Text,
            Mentions::new(),
            Some(&thread),
        )
        .unwrap();

        assert_eq!(
            payload,
            serde_json::json!({
                "msgtype": "m.text",
                "body": "Thread update",
                "format": "org.matrix.custom.html",
                "formatted_body": "<p>Thread <strong>update</strong></p>",
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

        let reply = RelatesTo::Reply {
            event_id: "$target:example.org".to_owned(),
        };
        let reply_payload = build_markdown_message_payload(
            "Inline reply",
            GatewayMessageType::Text,
            Mentions::new(),
            Some(&reply),
        )
        .unwrap();

        assert_eq!(
            reply_payload["m.relates_to"],
            serde_json::json!({
                "m.in_reply_to": {
                    "event_id": "$target:example.org"
                }
            })
        );
    }

    #[test]
    fn builds_full_matrix_replacement_payload_for_edits() {
        let mentions = resolve_mentions(Some(MentionsRequest {
            user_ids: Some(vec!["@alice:example.org".to_owned()]),
            room: Some(false),
        }))
        .unwrap();
        let edit = RelatesTo::Edit {
            event_id: "$original:example.org".to_owned(),
        };
        let payload = build_markdown_message_payload(
            "Updated **text**",
            GatewayMessageType::Text,
            mentions,
            Some(&edit),
        )
        .unwrap();

        assert_eq!(
            payload,
            serde_json::json!({
                "msgtype": "m.text",
                "body": "* Updated text",
                "format": "org.matrix.custom.html",
                "formatted_body": "* <p>Updated <strong>text</strong></p>",
                "m.mentions": {
                    "user_ids": ["@alice:example.org"]
                },
                "m.new_content": {
                    "msgtype": "m.text",
                    "body": "Updated text",
                    "format": "org.matrix.custom.html",
                    "formatted_body": "<p>Updated <strong>text</strong></p>",
                    "m.mentions": {
                        "user_ids": ["@alice:example.org"]
                    }
                },
                "m.relates_to": {
                    "rel_type": "m.replace",
                    "event_id": "$original:example.org"
                }
            })
        );
    }
}
