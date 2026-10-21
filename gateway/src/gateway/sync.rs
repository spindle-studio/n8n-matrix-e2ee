use futures_util::StreamExt;
use matrix_sdk::{
    Client, Room, RoomState,
    ruma::{
        OwnedUserId,
        events::{
            reaction::OriginalSyncReactionEvent,
            room::{
                member::{OriginalSyncRoomMemberEvent, StrippedRoomMemberEvent},
                message::OriginalSyncRoomMessageEvent,
            },
        },
    },
};
use tokio::sync::broadcast;
use tracing::{error, info, warn};

use crate::api::{
    GatewayEvent, gateway_message_type_label, reaction_content_to_gateway_event,
    room_member_event_to_gateway_event, room_message_content_to_gateway_event,
    stripped_room_invite_to_gateway_event,
};

pub(crate) async fn run_sync_loop(client: Client) {
    let mut sync_stream = Box::pin(
        client
            .sync_stream(matrix_sdk::config::SyncSettings::default())
            .await,
    );
    let mut sync_state = SyncLoopState::default();

    while let Some(result) = sync_stream.next().await {
        match result {
            Ok(_) => {
                if let Some(recovered_after_failures) = sync_state.record_success() {
                    info!(
                        recovered_after_failures,
                        "Matrix sync recovered after transient failures"
                    );
                }
            }
            Err(error) => {
                let consecutive_failures = sync_state.record_failure();
                warn!(
                    %error,
                    consecutive_failures,
                    "Matrix sync request failed; retrying"
                );
            }
        }
    }

    error!("Matrix sync stream ended unexpectedly");
}

pub(crate) fn register_event_handlers(
    client: &Client,
    events: broadcast::Sender<GatewayEvent>,
    own_user_id: OwnedUserId,
) {
    let message_events = events.clone();
    let message_own_user_id = own_user_id.clone();
    client.add_event_handler(move |event: OriginalSyncRoomMessageEvent, room: Room| {
        let events = message_events.clone();
        let own_user_id = message_own_user_id.clone();
        async move {
            info!(
                room_id = %room.room_id(),
                sender = %event.sender,
                event_id = %event.event_id,
                message_type = gateway_message_type_label(&event.content.msgtype),
                "received Matrix message metadata"
            );

            if is_self_authored_message(&event.sender, own_user_id.as_ref()) {
                info!(
                    room_id = %room.room_id(),
                    sender = %event.sender,
                    event_id = %event.event_id,
                    "ignoring self-authored Matrix message"
                );
                return;
            }

            let Some(gateway_event) = room_message_content_to_gateway_event(
                room.room_id().as_str(),
                event.sender.as_str(),
                event.event_id.as_str(),
                &event.content,
            ) else {
                return;
            };

            broadcast_event(&events, gateway_event);
        }
    });

    let reaction_events = events.clone();
    let reaction_own_user_id = own_user_id.clone();
    client.add_event_handler(move |event: OriginalSyncReactionEvent, room: Room| {
        let events = reaction_events.clone();
        let own_user_id = reaction_own_user_id.clone();
        async move {
            info!(
                room_id = %room.room_id(),
                sender = %event.sender,
                event_id = %event.event_id,
                target_event_id = %event.content.relates_to.event_id,
                "received Matrix reaction metadata"
            );

            if is_self_authored_message(&event.sender, own_user_id.as_ref()) {
                info!(
                    room_id = %room.room_id(),
                    sender = %event.sender,
                    event_id = %event.event_id,
                    "ignoring self-authored Matrix reaction"
                );
                return;
            }

            let Some(gateway_event) = reaction_content_to_gateway_event(
                room.room_id().as_str(),
                event.sender.as_str(),
                event.event_id.as_str(),
                &event.content,
            ) else {
                return;
            };

            broadcast_event(&events, gateway_event);
        }
    });

    let invite_events = events.clone();
    let invite_own_user_id = own_user_id.clone();
    client.add_event_handler(move |event: StrippedRoomMemberEvent, room: Room| {
        let events = invite_events.clone();
        let own_user_id = invite_own_user_id.clone();
        async move {
            if event.state_key != room.own_user_id() {
                return;
            }
            if room.state() != RoomState::Invited {
                return;
            }
            if is_self_authored_message(&event.sender, own_user_id.as_ref()) {
                return;
            }
            let Some(gateway_event) =
                stripped_room_invite_to_gateway_event(room.room_id().as_str(), &event)
            else {
                return;
            };

            info!(
                inviter = %event.sender,
                room_id = %room.room_id(),
                "received Matrix room invite for the gateway user"
            );

            broadcast_event(&events, gateway_event);
        }
    });

    let member_events = events;
    let member_own_user_id = own_user_id;
    client.add_event_handler(move |event: OriginalSyncRoomMemberEvent, room: Room| {
        let events = member_events.clone();
        let own_user_id = member_own_user_id.clone();
        async move {
            if room.state() != RoomState::Joined {
                return;
            }
            if is_self_authored_message(&event.sender, own_user_id.as_ref())
                || event.state_key == own_user_id
            {
                return;
            }
            let Some(gateway_event) =
                room_member_event_to_gateway_event(room.room_id().as_str(), &event)
            else {
                return;
            };

            info!(
                room_id = %room.room_id(),
                sender = %event.sender,
                user_id = %event.state_key,
                event_id = %event.event_id,
                membership = %event.content.membership,
                "received Matrix room member event"
            );

            broadcast_event(&events, gateway_event);
        }
    });
}

#[derive(Debug, Default)]
struct SyncLoopState {
    consecutive_failures: u32,
}

impl SyncLoopState {
    fn record_failure(&mut self) -> u32 {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.consecutive_failures
    }

    fn record_success(&mut self) -> Option<u32> {
        let consecutive_failures = self.consecutive_failures;
        self.consecutive_failures = 0;
        (consecutive_failures > 0).then_some(consecutive_failures)
    }
}

fn broadcast_event(events: &broadcast::Sender<GatewayEvent>, event: GatewayEvent) {
    if events.send(event).is_err() {
        tracing::debug!("received Matrix event with no WebSocket subscribers");
    }
}

fn is_self_authored_message(
    sender: &matrix_sdk::ruma::OwnedUserId,
    own_user_id: &matrix_sdk::ruma::UserId,
) -> bool {
    sender == own_user_id
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::ruma::{events::room::message::MessageType, owned_mxc_uri};

    #[test]
    fn message_type_label_reports_text_messages() {
        let message_type = MessageType::Text(
            matrix_sdk::ruma::events::room::message::TextMessageEventContent::plain("Hello"),
        );

        assert_eq!(gateway_message_type_label(&message_type), "text");
    }

    #[test]
    fn message_type_label_reports_notice_messages() {
        let message_type = MessageType::Notice(
            matrix_sdk::ruma::events::room::message::NoticeMessageEventContent::plain("Notice"),
        );

        assert_eq!(gateway_message_type_label(&message_type), "notice");
    }

    #[test]
    fn message_type_label_reports_emote_messages() {
        let message_type = MessageType::Emote(
            matrix_sdk::ruma::events::room::message::EmoteMessageEventContent::plain("waves"),
        );

        assert_eq!(gateway_message_type_label(&message_type), "emote");
    }

    #[test]
    fn message_type_label_reports_image_messages() {
        let message_type = MessageType::Image(
            matrix_sdk::ruma::events::room::message::ImageMessageEventContent::plain(
                "cat.png".to_owned(),
                owned_mxc_uri!("mxc://example.org/cat"),
            ),
        );

        assert_eq!(gateway_message_type_label(&message_type), "image");
    }

    #[test]
    fn message_type_label_reports_audio_messages() {
        let message_type = MessageType::Audio(
            matrix_sdk::ruma::events::room::message::AudioMessageEventContent::plain(
                "audio.ogg".to_owned(),
                owned_mxc_uri!("mxc://example.org/audio"),
            ),
        );

        assert_eq!(gateway_message_type_label(&message_type), "audio");
    }

    #[test]
    fn self_authored_message_check_matches_exact_sender() {
        let own_user_id: OwnedUserId = "@bot:example.org"
            .parse()
            .expect("own user ID should parse");
        let sender: OwnedUserId = "@bot:example.org".parse().expect("sender should parse");
        let other_sender: OwnedUserId = "@user:example.org"
            .parse()
            .expect("other sender should parse");

        assert!(is_self_authored_message(&sender, own_user_id.as_ref()));
        assert!(!is_self_authored_message(
            &other_sender,
            own_user_id.as_ref()
        ));
    }

    #[test]
    fn sync_loop_state_tracks_failures_until_recovery() {
        let mut state = SyncLoopState::default();

        assert_eq!(state.record_failure(), 1);
        assert_eq!(state.record_failure(), 2);
        assert_eq!(state.record_success(), Some(2));
        assert_eq!(state.record_success(), None);
    }
}
