use axum::{Json, extract::State, http::HeaderMap};

use crate::{api::error::ApiResult, gateway::AppState};

use super::{
    MessageActionResponse,
    event::GatewayMessageType,
    text::{SendMarkdownRequest, send_markdown_message},
};

pub(super) async fn send_notice(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SendMarkdownRequest>,
) -> ApiResult<Json<MessageActionResponse>> {
    send_markdown_message(state, headers, request, GatewayMessageType::Notice).await
}

#[cfg(test)]
mod tests {
    use matrix_sdk::ruma::events::Mentions;

    use super::super::text::build_markdown_message_content;
    use super::*;

    #[test]
    fn converts_notice_markdown_to_plain_body_and_html_formatted_body() {
        let content = build_markdown_message_content(
            "Silent **status** and `code`!",
            GatewayMessageType::Notice,
            Mentions::new(),
        )
        .unwrap();
        let json = serde_json::to_value(content).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "msgtype": "m.notice",
                "body": "Silent status and code!",
                "format": "org.matrix.custom.html",
                "formatted_body": "<p>Silent <strong>status</strong> and <code>code</code>!</p>",
                "m.mentions": {}
            })
        );
    }

    #[test]
    fn rejects_empty_or_whitespace_only_notice_body() {
        assert!(
            build_markdown_message_content("", GatewayMessageType::Notice, Mentions::new())
                .is_none()
        );
        assert!(
            build_markdown_message_content(
                "   \n\t  ",
                GatewayMessageType::Notice,
                Mentions::new()
            )
            .is_none()
        );
    }
}
