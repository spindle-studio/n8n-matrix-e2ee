use std::{net::SocketAddr, time::Duration};

use axum::{
    extract::{
        ConnectInfo, State,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use futures_util::SinkExt;
use serde::Deserialize;
use tokio::{
    sync::{Semaphore, broadcast},
    time::timeout,
};
use tracing::{error, info, warn};

use crate::gateway::AppState;

use super::GatewayEvent;

const AUTH_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CLIENT_MESSAGE_SIZE: usize = 4096;
static PENDING_AUTH: Semaphore = Semaphore::const_new(64);

pub(crate) async fn websocket(
    ws: WebSocketUpgrade,
    ConnectInfo(remote_addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let user_agent = request_user_agent(&headers);
    let Ok(permit) = PENDING_AUTH.try_acquire() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    ws.max_message_size(MAX_CLIENT_MESSAGE_SIZE)
        .max_frame_size(MAX_CLIENT_MESSAGE_SIZE)
        .on_upgrade(move |mut socket| async move {
            let account = authenticate(
                &mut socket,
                |token| state.account_for_token(token).cloned(),
                AUTH_TIMEOUT,
            )
            .await;
            drop(permit);
            if let Some(account) = account {
                forward_events(
                    socket,
                    account.events.subscribe(),
                    &account.account_id,
                    remote_addr,
                    user_agent,
                )
                .await;
            }
        })
        .into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Authentication {
    #[serde(rename = "type")]
    kind: String,
    token: String,
}

async fn authenticate<T, F>(
    socket: &mut WebSocket,
    resolve_token: F,
    deadline: Duration,
) -> Option<T>
where
    F: FnOnce(&str) -> Option<T>,
{
    let result = timeout(deadline, async {
        loop {
            match socket.recv().await {
                Some(Ok(Message::Text(payload))) => {
                    let Ok(auth) = serde_json::from_str::<Authentication>(&payload) else {
                        return None;
                    };
                    if auth.kind != "authenticate" {
                        return None;
                    }
                    return resolve_token(&auth.token);
                }
                Some(Ok(Message::Ping(_))) => {
                    if socket.flush().await.is_err() {
                        return None;
                    }
                }
                Some(Ok(Message::Pong(_))) => {}
                _ => return None,
            }
        }
    })
    .await;
    let Ok(Some(authenticated)) = result else {
        let _ = timeout(
            Duration::from_secs(1),
            socket.send(Message::Close(Some(CloseFrame {
                code: 4401,
                reason: "Authentication failed".into(),
            }))),
        )
        .await;
        return None;
    };
    Some(authenticated)
}

fn request_user_agent(headers: &HeaderMap) -> String {
    headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("unknown")
        .to_owned()
}

async fn forward_events(
    mut socket: WebSocket,
    mut events: broadcast::Receiver<GatewayEvent>,
    account_id: &str,
    remote_addr: SocketAddr,
    user_agent: String,
) {
    // The subscription already exists before acknowledging authentication, so
    // an event arriving immediately after the acknowledgement is not missed.
    if !matches!(
        timeout(
            Duration::from_secs(1),
            socket.send(Message::Text(r#"{"type":"authenticated"}"#.into(),))
        )
        .await,
        Ok(Ok(()))
    ) {
        return;
    }
    info!(account = %account_id, remote_addr = %remote_addr, user_agent, "WebSocket client connected to Matrix event stream");
    loop {
        tokio::select! {
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Ping(_))) => {
                    // Reading queues Axum's automatic Pong; flush it even when
                    // no Matrix events are available to drive another write.
                    if socket.flush().await.is_err() {
                        return;
                    }
                }
                Some(Ok(Message::Close(_))) => {
                    // Flush the automatic close response before dropping the socket.
                    let _ = socket.flush().await;
                    return;
                }
                Some(Err(_)) | None => return,
                // This is an outgoing event stream, so application messages
                // from clients have no effect.
                Some(Ok(_)) => {}
            },
            event = events.recv() => match event {
                Ok(event) => match serde_json::to_string(&event) {
                    Ok(payload) => {
                        if socket.send(Message::Text(payload.into())).await.is_err() {
                            return;
                        }
                    }
                    Err(error) => error!(account = %account_id, %error, "failed to serialize Matrix event"),
                },
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!(account = %account_id, skipped, remote_addr = %remote_addr, user_agent, "WebSocket client lagged behind Matrix event stream")
                }
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::get};
    use futures_util::StreamExt;
    use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
    use tokio::{
        net::TcpStream,
        task::JoinHandle,
        time::{Duration, timeout},
    };
    use tokio_tungstenite::{
        MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message as ClientMessage,
    };

    type ClientSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

    struct TestServer {
        address: SocketAddr,
        events: broadcast::Sender<GatewayEvent>,
        task: JoinHandle<()>,
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    impl TestServer {
        async fn start() -> Self {
            let (events, _) = broadcast::channel(256);
            let sender = events.clone();
            let app = Router::new().route(
                "/ws",
                get(move |ws: WebSocketUpgrade| {
                    let sender = sender.clone();
                    async move {
                        ws.max_message_size(MAX_CLIENT_MESSAGE_SIZE)
                            .max_frame_size(MAX_CLIENT_MESSAGE_SIZE)
                            .on_upgrade(move |mut socket| async move {
                                let Some(sender) = authenticate(
                                    &mut socket,
                                    |token| (token == "test-token").then(|| sender.clone()),
                                    Duration::from_millis(200),
                                )
                                .await
                                else {
                                    return;
                                };
                                forward_events(
                                    socket,
                                    sender.subscribe(),
                                    "test-account",
                                    "127.0.0.1:0".parse().unwrap(),
                                    "test".into(),
                                )
                                .await
                            })
                    }
                }),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let task = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            Self {
                address,
                events,
                task,
            }
        }

        async fn connect_raw(&self) -> ClientSocket {
            timeout(
                Duration::from_secs(5),
                connect_async(format!("ws://{}/ws", self.address)),
            )
            .await
            .unwrap()
            .unwrap()
            .0
        }

        async fn connect(&self) -> ClientSocket {
            let mut socket = self.connect_raw().await;
            socket
                .send(ClientMessage::Text(
                    r#"{"type":"authenticate","token":"test-token"}"#.into(),
                ))
                .await
                .unwrap();
            assert_eq!(
                receive(&mut socket).await,
                ClientMessage::Text(r#"{"type":"authenticated"}"#.into())
            );
            socket
        }

        async fn wait_for_subscribers(&self, count: usize) {
            timeout(Duration::from_secs(5), async {
                while self.events.receiver_count() != count {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("connection subscription was not cleaned up");
        }
    }

    async fn receive(socket: &mut ClientSocket) -> ClientMessage {
        timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("timed out waiting for WebSocket frame")
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn unauthenticated_clients_never_subscribe_and_time_out() {
        let server = TestServer::start().await;
        let mut socket = server.connect_raw().await;
        assert_eq!(server.events.receiver_count(), 0);
        assert!(
            matches!(receive(&mut socket).await, ClientMessage::Close(Some(frame)) if frame.code == 4401.into())
        );
        assert_eq!(server.events.receiver_count(), 0);
    }

    #[tokio::test]
    async fn invalid_first_messages_are_rejected() {
        let server = TestServer::start().await;
        for payload in [
            r#"{"type":"authenticate","token":"wrong"}"#,
            r#"{"type":"hello"}"#,
            "not json",
        ] {
            let mut socket = server.connect_raw().await;
            socket
                .send(ClientMessage::Text(payload.into()))
                .await
                .unwrap();
            assert!(
                matches!(receive(&mut socket).await, ClientMessage::Close(Some(frame)) if frame.code == 4401.into())
            );
            assert_eq!(server.events.receiver_count(), 0);
        }
    }

    #[tokio::test]
    async fn bearer_header_alone_does_not_authenticate() {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let server = TestServer::start().await;
        let mut request = format!("ws://{}/ws", server.address)
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("Authorization", "Bearer test-token".parse().unwrap());
        let (mut client, _) = connect_async(request).await.unwrap();
        assert_eq!(server.events.receiver_count(), 0);
        assert!(
            matches!(receive(&mut client).await, ClientMessage::Close(Some(frame)) if frame.code == 4401.into())
        );
    }

    #[tokio::test]
    async fn binary_authentication_and_oversized_messages_never_subscribe() {
        let server = TestServer::start().await;
        for message in [
            ClientMessage::Binary(vec![1, 2, 3].into()),
            ClientMessage::Text("x".repeat(MAX_CLIENT_MESSAGE_SIZE + 1).into()),
        ] {
            let mut socket = server.connect_raw().await;
            socket.send(message).await.unwrap();
            // Oversize frames may close the transport rather than send a close frame.
            let _ = timeout(Duration::from_secs(5), socket.next())
                .await
                .unwrap();
            assert_eq!(server.events.receiver_count(), 0);
        }
    }

    #[tokio::test]
    async fn idle_connection_answers_ping_and_completes_close() {
        let server = TestServer::start().await;
        let mut client = server.connect().await;
        client
            .send(ClientMessage::Ping(b"heartbeat".to_vec().into()))
            .await
            .unwrap();
        assert_eq!(
            receive(&mut client).await,
            ClientMessage::Pong(b"heartbeat".to_vec().into())
        );
        client.send(ClientMessage::Close(None)).await.unwrap();
        assert!(matches!(
            receive(&mut client).await,
            ClientMessage::Close(_)
        ));
        server.wait_for_subscribers(0).await;
    }

    #[tokio::test]
    async fn abrupt_disconnect_releases_idle_subscription() {
        let server = TestServer::start().await;
        let client = server.connect().await;
        assert_eq!(server.events.receiver_count(), 1);
        drop(client);
        server.wait_for_subscribers(0).await;
    }

    #[tokio::test]
    async fn clients_receive_same_events_after_another_client_disconnects() {
        let server = TestServer::start().await;
        let mut first = server.connect().await;
        let mut second = server.connect().await;
        let third = server.connect().await;
        drop(third);
        server.wait_for_subscribers(2).await;
        first
            .send(ClientMessage::Ping(vec![1, 2, 3].into()))
            .await
            .unwrap();
        assert!(matches!(receive(&mut first).await, ClientMessage::Pong(_)));
        let event = crate::api::room_message_content_to_gateway_event(
            "!room:example.org",
            "@user:example.org",
            "$event",
            &RoomMessageEventContent::text_plain("hello"),
        )
        .unwrap();
        let expected = serde_json::to_string(&event).unwrap();
        assert_eq!(server.events.send(event).unwrap(), 2);
        assert_eq!(
            receive(&mut first).await,
            ClientMessage::Text(expected.clone().into())
        );
        assert_eq!(
            receive(&mut second).await,
            ClientMessage::Text(expected.into())
        );
    }

    #[tokio::test]
    async fn clients_receive_events_for_their_authenticated_account_only() {
        let (support_events, _) = broadcast::channel(256);
        let (alerts_events, _) = broadcast::channel(256);
        let support_sender = support_events.clone();
        let alerts_sender = alerts_events.clone();

        let app = Router::new().route(
            "/ws",
            get(move |ws: WebSocketUpgrade| {
                let support_sender = support_sender.clone();
                let alerts_sender = alerts_sender.clone();
                async move {
                    ws.max_message_size(MAX_CLIENT_MESSAGE_SIZE)
                        .max_frame_size(MAX_CLIENT_MESSAGE_SIZE)
                        .on_upgrade(move |mut socket| async move {
                            let Some((account_id, sender)) = authenticate(
                                &mut socket,
                                |token| match token {
                                    "support-token" => Some(("support", support_sender.clone())),
                                    "alerts-token" => Some(("alerts", alerts_sender.clone())),
                                    _ => None,
                                },
                                Duration::from_millis(200),
                            )
                            .await
                            else {
                                return;
                            };
                            forward_events(
                                socket,
                                sender.subscribe(),
                                account_id,
                                "127.0.0.1:0".parse().unwrap(),
                                "test".into(),
                            )
                            .await
                        })
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let (mut support_client, _) =
            connect_async(format!("ws://{address}/ws")).await.unwrap();
        support_client
            .send(ClientMessage::Text(
                r#"{"type":"authenticate","token":"support-token"}"#.into(),
            ))
            .await
            .unwrap();
        assert_eq!(
            receive(&mut support_client).await,
            ClientMessage::Text(r#"{"type":"authenticated"}"#.into())
        );

        let (mut alerts_client, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
        alerts_client
            .send(ClientMessage::Text(
                r#"{"type":"authenticate","token":"alerts-token"}"#.into(),
            ))
            .await
            .unwrap();
        assert_eq!(
            receive(&mut alerts_client).await,
            ClientMessage::Text(r#"{"type":"authenticated"}"#.into())
        );

        let support_event = crate::api::room_message_content_to_gateway_event(
            "!support:example.org",
            "@user:example.org",
            "$support-event",
            &RoomMessageEventContent::text_plain("support message"),
        )
        .unwrap();
        let alerts_event = crate::api::room_message_content_to_gateway_event(
            "!alerts:example.org",
            "@user:example.org",
            "$alerts-event",
            &RoomMessageEventContent::text_plain("alerts message"),
        )
        .unwrap();

        let expected_support = serde_json::to_string(&support_event).unwrap();
        let expected_alerts = serde_json::to_string(&alerts_event).unwrap();

        assert_eq!(support_events.send(support_event).unwrap(), 1);
        assert_eq!(alerts_events.send(alerts_event).unwrap(), 1);

        assert_eq!(
            receive(&mut support_client).await,
            ClientMessage::Text(expected_support.into())
        );
        assert_eq!(
            receive(&mut alerts_client).await,
            ClientMessage::Text(expected_alerts.into())
        );

        task.abort();
    }
}
