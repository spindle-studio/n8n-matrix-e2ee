use axum::{Json, extract::State, http::HeaderMap};

use crate::{api::error::ApiResult, gateway::AppState};

use super::{
    MessageActionResponse,
    event::GatewayMessageType,
    text::{SendMarkdownRequest, send_markdown_message},
};

pub(super) async fn send_emote(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SendMarkdownRequest>,
) -> ApiResult<Json<MessageActionResponse>> {
    send_markdown_message(state, headers, request, GatewayMessageType::Emote).await
}

#[cfg(test)]
mod tests {
    use matrix_sdk::ruma::events::Mentions;

    use super::super::text::build_markdown_message_content;
    use super::*;

    #[test]
    fn converts_emote_markdown_to_plain_body_and_html_formatted_body() {
        let content = build_markdown_message_content(
            "waves **excitedly** with `code`!",
            GatewayMessageType::Emote,
            Mentions::new(),
        )
        .unwrap();
        let json = serde_json::to_value(content).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "msgtype": "m.emote",
                "body": "waves excitedly with code!",
                "format": "org.matrix.custom.html",
                "formatted_body": "<p>waves <strong>excitedly</strong> with <code>code</code>!</p>",
                "m.mentions": {}
            })
        );
    }

    #[test]
    fn rejects_empty_or_whitespace_only_emote_body() {
        assert!(
            build_markdown_message_content("", GatewayMessageType::Emote, Mentions::new())
                .is_none()
        );
        assert!(
            build_markdown_message_content(
                "   \n\t  ",
                GatewayMessageType::Emote,
                Mentions::new()
            )
            .is_none()
        );
    }
}
