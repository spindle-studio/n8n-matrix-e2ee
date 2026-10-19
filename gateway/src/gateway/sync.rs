use std::collections::HashSet;

use futures_util::StreamExt;
use matrix_sdk::{
    Client, Room, RoomState,
    ruma::{
        OwnedUserId, events::room::member::StrippedRoomMemberEvent,
        events::room::message::OriginalSyncRoomMessageEvent,
    },
};
use tokio::sync::broadcast;
use tracing::{error, info, warn};

use crate::api::{GatewayEvent, gateway_message_type_label, room_message_content_to_gateway_event};

pub(crate) async fn run_sync_loop(client: Client, allowed_inviters: HashSet<String>) {
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

                if sync_state.mark_startup_reconciled() {
                    reconcile_pending_invites(&client, &allowed_inviters).await;
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
    allowed_inviters: HashSet<String>,
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

    client.add_event_handler(move |event: StrippedRoomMemberEvent, room: Room| {
        let allowed_inviters = allowed_inviters.clone();
        async move {
            if event.state_key != room.own_user_id() {
                return;
            }
            if room.state() != RoomState::Invited {
                return;
            }
            let inviter = event.sender.as_str();

            info!(
                inviter,
                room_id = %room.room_id(),
                invite_source = "sync_event",
                "received Matrix room invite for the gateway user"
            );

            handle_invited_room(
                &room,
                authorize_inviter(&allowed_inviters, Some(inviter)),
                "sync_event",
            )
            .await;
        }
    });
}

#[derive(Debug, Default)]
struct SyncLoopState {
    startup_reconciled: bool,
    consecutive_failures: u32,
}

impl SyncLoopState {
    fn mark_startup_reconciled(&mut self) -> bool {
        if self.startup_reconciled {
            false
        } else {
            self.startup_reconciled = true;
            true
        }
    }

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

async fn reconcile_pending_invites(client: &Client, allowed_inviters: &HashSet<String>) {
    let invited_rooms = client.invited_rooms();

    if invited_rooms.is_empty() {
        info!("no pending Matrix invites required startup reconciliation");
        return;
    }

    info!(
        pending_invite_count = invited_rooms.len(),
        "reconciling pending Matrix invites after startup"
    );

    for room in invited_rooms {
        let invite_authorization = match room.invite_details().await {
            Ok(details) => authorize_inviter(allowed_inviters, Some(details.inviter_id.as_str())),
            Err(error) => {
                error!(
                    %error,
                    room_id = %room.room_id(),
                    invite_source = "startup_reconciliation",
                    "failed to inspect pending Matrix invite"
                );
                authorize_inviter(allowed_inviters, None)
            }
        };

        handle_invited_room(&room, invite_authorization, "startup_reconciliation").await;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InviteAuthorization {
    Authorized { inviter: String },
    Unauthorized { inviter: String },
    Unverifiable,
}

fn authorize_inviter(
    allowed_inviters: &HashSet<String>,
    inviter: Option<&str>,
) -> InviteAuthorization {
    match inviter {
        Some(inviter) if is_allowed_inviter(allowed_inviters, inviter) => {
            InviteAuthorization::Authorized {
                inviter: inviter.to_owned(),
            }
        }
        Some(inviter) => InviteAuthorization::Unauthorized {
            inviter: inviter.to_owned(),
        },
        None => InviteAuthorization::Unverifiable,
    }
}

async fn handle_invited_room(
    room: &Room,
    invite_authorization: InviteAuthorization,
    invite_source: &'static str,
) {
    match invite_authorization {
        InviteAuthorization::Authorized { inviter } => {
            info!(
                inviter,
                room_id = %room.room_id(),
                invite_source,
                "accepting Matrix invite from authorized user"
            );

            if let Err(error) = room.join().await {
                error!(
                    %error,
                    inviter,
                    room_id = %room.room_id(),
                    invite_source,
                    "failed to accept Matrix invite"
                );
            } else {
                info!(
                    inviter,
                    room_id = %room.room_id(),
                    invite_source,
                    "joined Matrix room after accepting invite"
                );
            }
        }
        InviteAuthorization::Unauthorized { inviter } => {
            reject_invited_room(room, Some(&inviter), invite_source).await;
        }
        InviteAuthorization::Unverifiable => {
            reject_invited_room(room, None, invite_source).await;
        }
    }
}

async fn reject_invited_room(room: &Room, inviter: Option<&str>, invite_source: &'static str) {
    match inviter {
        Some(inviter) => warn!(
            inviter,
            room_id = %room.room_id(),
            invite_source,
            "rejecting Matrix invite from unauthorized user"
        ),
        None => warn!(
            room_id = %room.room_id(),
            invite_source,
            "rejecting Matrix invite because the inviter could not be verified"
        ),
    }

    if let Err(error) = room.leave().await {
        match inviter {
            Some(inviter) => error!(
                %error,
                inviter,
                room_id = %room.room_id(),
                invite_source,
                "failed to reject Matrix invite"
            ),
            None => error!(
                %error,
                room_id = %room.room_id(),
                invite_source,
                "failed to reject Matrix invite with an unverifiable inviter"
            ),
        }
    }
}

fn broadcast_event(events: &broadcast::Sender<GatewayEvent>, event: GatewayEvent) {
    if events.send(event).is_err() {
        tracing::debug!("received Matrix event with no WebSocket subscribers");
    }
}

fn is_allowed_inviter(allowed_inviters: &HashSet<String>, inviter: &str) -> bool {
    allowed_inviters.contains(inviter)
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
    fn allowed_inviter_check_matches_exact_user_id() {
        let inviters = HashSet::from([
            "@alice:example.org".to_owned(),
            "@bob:example.org".to_owned(),
        ]);

        assert!(is_allowed_inviter(&inviters, "@alice:example.org"));
        assert!(!is_allowed_inviter(&inviters, "@charlie:example.org"));
    }

    #[test]
    fn authorized_inviter_is_accepted() {
        let inviters = HashSet::from([
            "@alice:example.org".to_owned(),
            "@bob:example.org".to_owned(),
        ]);

        assert_eq!(
            authorize_inviter(&inviters, Some("@alice:example.org")),
            InviteAuthorization::Authorized {
                inviter: "@alice:example.org".to_owned()
            }
        );
    }

    #[test]
    fn unauthorized_inviter_is_rejected() {
        let inviters = HashSet::from([
            "@alice:example.org".to_owned(),
            "@bob:example.org".to_owned(),
        ]);

        assert_eq!(
            authorize_inviter(&inviters, Some("@charlie:example.org")),
            InviteAuthorization::Unauthorized {
                inviter: "@charlie:example.org".to_owned()
            }
        );
    }

    #[test]
    fn unverifiable_pending_invites_are_rejected() {
        let inviters = HashSet::from([
            "@alice:example.org".to_owned(),
            "@bob:example.org".to_owned(),
        ]);

        assert_eq!(
            authorize_inviter(&inviters, None),
            InviteAuthorization::Unverifiable
        );
    }

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
    fn sync_loop_state_only_reconciles_startup_once() {
        let mut state = SyncLoopState::default();

        assert!(state.mark_startup_reconciled());
        assert!(!state.mark_startup_reconciled());
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
