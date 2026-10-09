use matrix_sdk::{
    Client, Room,
    ruma::{OwnedEventId, OwnedRoomId},
};

use super::error::{ApiResult, client_error};

pub(crate) fn parse_room_id(room_id: &str) -> ApiResult<OwnedRoomId> {
    if room_id.trim().is_empty() {
        return Err(client_error("room_id must not be empty"));
    }

    room_id
        .parse()
        .map_err(|_| client_error("room_id must be a valid Matrix room ID"))
}

pub(crate) fn parse_event_id(event_id: &str) -> ApiResult<OwnedEventId> {
    if event_id.trim().is_empty() {
        return Err(client_error("event_id must not be empty"));
    }

    event_id
        .parse()
        .map_err(|_| client_error("event_id must be a valid Matrix event ID"))
}

pub(crate) fn get_known_room(client: &Client, room_id: &OwnedRoomId) -> ApiResult<Room> {
    client
        .get_room(room_id)
        .ok_or_else(|| client_error("gateway does not know this room"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_id_parser_rejects_empty_and_invalid_ids() {
        assert!(parse_room_id("").is_err());
        assert!(parse_room_id("not-a-room-id").is_err());
        assert_eq!(
            parse_room_id("!room:example.org").unwrap().as_str(),
            "!room:example.org"
        );
    }

    #[test]
    fn event_id_parser_rejects_empty_and_invalid_ids() {
        assert!(parse_event_id("").is_err());
        assert!(parse_event_id("not-an-event-id").is_err());
        assert_eq!(
            parse_event_id("$event:example.org").unwrap().as_str(),
            "$event:example.org"
        );
    }
}
