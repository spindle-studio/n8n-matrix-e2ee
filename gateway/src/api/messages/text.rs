use axum::{Json, extract::State, http::HeaderMap};
use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
use pulldown_cmark::{Event, Options, Parser, TagEnd, html};
use serde::Deserialize;
use tracing::{error, info};

use crate::gateway::AppState;

use super::{MessageActionResponse, event::GatewayMessageType, message_action_response};
use crate::api::{
    auth::authorize_request,
    error::{ApiResult, client_error, server_error},
    room::{get_known_room, parse_room_id},
};

#[derive(Debug, Deserialize)]
pub(super) struct SendMarkdownRequest {
    room_id: String,
    body: String,
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
    let content = build_markdown_message_content(&request.body, gateway_message_type)
        .ok_or_else(|| client_error("body must not be empty"))?;

    let room_id = parse_room_id(&request.room_id)?;
    let room = get_known_room(&state.client, &room_id)?;
    info!(
        room_id = %room_id,
        message_type = gateway_message_type.as_str(),
        "sending Matrix message"
    );
    let response = room.send(content).await.map_err(|error| {
        error!(%error, room_id = %room_id, "failed to send Matrix message");
        server_error("failed to send Matrix message")
    })?;

    Ok(message_action_response(
        room_id.to_string(),
        response.response.event_id.to_string(),
        gateway_message_type,
    ))
}

pub(super) fn build_markdown_message_content(
    markdown: &str,
    gateway_message_type: GatewayMessageType,
) -> Option<RoomMessageEventContent> {
    let (plain_body, formatted_body) = compile_markdown(markdown)?;
    match gateway_message_type {
        GatewayMessageType::Text => Some(RoomMessageEventContent::text_html(
            plain_body,
            formatted_body,
        )),
        GatewayMessageType::Notice => Some(RoomMessageEventContent::notice_html(
            plain_body,
            formatted_body,
        )),
        GatewayMessageType::Image | GatewayMessageType::Audio => None,
    }
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
        let content =
            build_markdown_message_content("Hello **world** and `code`!", GatewayMessageType::Text)
                .unwrap();
        let json = serde_json::to_value(content).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "msgtype": "m.text",
                "body": "Hello world and code!",
                "format": "org.matrix.custom.html",
                "formatted_body": "<p>Hello <strong>world</strong> and <code>code</code>!</p>"
            })
        );
    }

    #[test]
    fn converts_plain_text_to_body_and_html_formatted_body() {
        let content =
            build_markdown_message_content("Hello Matrix", GatewayMessageType::Text).unwrap();
        let json = serde_json::to_value(content).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "msgtype": "m.text",
                "body": "Hello Matrix",
                "format": "org.matrix.custom.html",
                "formatted_body": "<p>Hello Matrix</p>"
            })
        );
    }

    #[test]
    fn rejects_empty_or_whitespace_only_body() {
        assert!(build_markdown_message_content("", GatewayMessageType::Text).is_none());
        assert!(build_markdown_message_content("   \n\t  ", GatewayMessageType::Text).is_none());
    }
}
