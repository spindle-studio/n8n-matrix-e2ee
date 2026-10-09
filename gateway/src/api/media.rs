use axum::{
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
};
use matrix_sdk::{
    deserialized_responses::TimelineEvent,
    media::{MediaFormat, MediaRequestParameters},
    ruma::events::{
        AnySyncMessageLikeEvent, AnySyncTimelineEvent,
        room::{
            MediaSource,
            message::{AudioMessageEventContent, ImageMessageEventContent, MessageType},
        },
    },
};
use serde::Deserialize;
use tracing::error;

use crate::gateway::AppState;

use super::{
    auth::authorize_request,
    error::{ApiResult, client_error, server_error},
    room::{get_known_room, parse_event_id, parse_room_id},
};

#[derive(Debug, Deserialize)]
pub(super) struct MediaDownloadQuery {
    room_id: String,
    event_id: String,
}

enum DownloadableMedia {
    Image(ImageMessageEventContent),
    Audio(AudioMessageEventContent),
}

impl DownloadableMedia {
    fn mime_type(&self) -> &str {
        match self {
            Self::Image(content) => content
                .info
                .as_ref()
                .and_then(|info| info.mimetype.as_deref())
                .unwrap_or("application/octet-stream"),
            Self::Audio(content) => content
                .info
                .as_ref()
                .and_then(|info| info.mimetype.as_deref())
                .unwrap_or("audio/ogg"),
        }
    }

    fn file_name(&self) -> &str {
        match self {
            Self::Image(content) => content.filename(),
            Self::Audio(content) => content.filename(),
        }
    }

    fn source(&self) -> MediaSource {
        match self {
            Self::Image(content) => content.source.clone(),
            Self::Audio(content) => content.source.clone(),
        }
    }
}

pub(super) async fn download_media(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<MediaDownloadQuery>,
) -> ApiResult<Response> {
    authorize_request(&state.api_secret_token, &headers)?;
    let room_id = parse_room_id(&query.room_id)?;
    let event_id = parse_event_id(&query.event_id)?;
    let room = get_known_room(&state.client, &room_id)?;
    let event = room.event(&event_id, None).await.map_err(|error| {
        error!(%error, room_id = %room_id, event_id = %event_id, "failed to fetch Matrix event");
        server_error("failed to fetch Matrix event")
    })?;
    let media = downloadable_media_from_event(&event)
        .ok_or_else(|| client_error("event does not contain downloadable media"))?;
    let request = MediaRequestParameters {
        source: media.source(),
        format: MediaFormat::File,
    };
    let bytes = state.client.media().get_media_content(&request, true).await.map_err(|error| {
        error!(%error, room_id = %room_id, event_id = %event_id, "failed to download Matrix media");
        server_error("failed to download Matrix media")
    })?;
    Ok((
        media_response_headers(media.mime_type(), Some(media.file_name())),
        bytes,
    )
        .into_response())
}

fn downloadable_media_from_event(event: &TimelineEvent) -> Option<DownloadableMedia> {
    let timeline_event: AnySyncTimelineEvent = event.raw().deserialize().ok()?;
    match timeline_event {
        AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomMessage(message_event)) => {
            match &message_event.as_original()?.content.msgtype {
                matrix_sdk::ruma::events::room::message::MessageType::Image(content) => {
                    Some(DownloadableMedia::Image(content.clone()))
                }
                MessageType::Audio(content) => Some(DownloadableMedia::Audio(content.clone())),
                _ => None,
            }
        }
        _ => None,
    }
}

fn media_response_headers(mime_type: &str, file_name: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    if let Some(file_name) = file_name
        .map(sanitize_content_disposition_filename)
        .filter(|file_name| !file_name.is_empty())
        && let Ok(disposition) = HeaderValue::from_str(&format!("inline; filename=\"{file_name}\""))
    {
        headers.insert(header::CONTENT_DISPOSITION, disposition);
    }
    headers
}

fn sanitize_content_disposition_filename(file_name: &str) -> String {
    file_name
        .chars()
        .map(|character| match character {
            '"' | '\\' | '\r' | '\n' => '_',
            _ => character,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use axum::http::header;

    use super::*;

    #[test]
    fn media_headers_sanitize_content_disposition_file_names() {
        let headers = media_response_headers("image/png", Some("cat\"\r\n.png"));

        assert_eq!(headers.get(header::CONTENT_TYPE).unwrap(), "image/png");
        assert_eq!(
            headers.get(header::CONTENT_DISPOSITION).unwrap(),
            "inline; filename=\"cat___.png\""
        );
    }
}
