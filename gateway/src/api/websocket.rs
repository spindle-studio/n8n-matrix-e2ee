use std::net::SocketAddr;

use axum::{
    extract::{
        ConnectInfo, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::HeaderMap,
    response::IntoResponse,
};
use futures_util::SinkExt;
use tokio::sync::broadcast;
use tracing::{error, info, warn};

use crate::gateway::AppState;

use super::{GatewayEvent, auth::authorize_request};

pub(crate) async fn websocket(
    ws: WebSocketUpgrade,
    ConnectInfo(remote_addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let user_agent = request_user_agent(&headers);
    if let Err(error) = authorize_request(&state.api_secret_token, &headers) {
        warn!(remote_addr = %remote_addr, user_agent, "rejecting unauthorized WebSocket connection");
        return error.into_response();
    }
    ws.on_upgrade(move |socket| {
        forward_events(socket, state.events.subscribe(), remote_addr, user_agent)
    })
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
    remote_addr: SocketAddr,
    user_agent: String,
) {
    info!(remote_addr = %remote_addr, user_agent, "WebSocket client connected to Matrix event stream");
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
                    Err(error) => error!(%error, "failed to serialize Matrix event"),
                },
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!(skipped, remote_addr = %remote_addr, user_agent, "WebSocket client lagged behind Matrix event stream")
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
                    let receiver = sender.subscribe();
                    async move {
                        ws.on_upgrade(move |socket| {
                            forward_events(
                                socket,
                                receiver,
                                "127.0.0.1:0".parse().unwrap(),
                                "test".into(),
                            )
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

        async fn connect(&self) -> ClientSocket {
            timeout(
                Duration::from_secs(5),
                connect_async(format!("ws://{}/ws", self.address)),
            )
            .await
            .unwrap()
            .unwrap()
            .0
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
}
