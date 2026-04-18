use axum::{
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use spinbike_core::ws::ClientMsg;
use tracing::{info, warn};

use crate::AppState;
use crate::auth;

#[derive(Deserialize)]
pub struct WsQuery {
    pub token: Option<String>,
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
) -> impl IntoResponse {
    // M2: Optional authentication via token query parameter.
    let claims = query
        .token
        .as_deref()
        .and_then(|t| auth::validate_token(&state.jwt_secret, t).ok());

    if let Some(ref c) = claims {
        info!(
            user_id = c.sub,
            email = %c.email,
            "Authenticated WebSocket connection"
        );
    } else {
        info!("Anonymous WebSocket connection");
    }

    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();

    // Subscribe to broadcast channel.
    let mut broadcast_rx = state.event_tx.subscribe();

    // Spawn a task to forward broadcast messages to the WebSocket client.
    let send_task = tokio::spawn(async move {
        while let Ok(msg) = broadcast_rx.recv().await {
            let json = match serde_json::to_string(&msg) {
                Ok(j) => j,
                Err(e) => {
                    warn!("Failed to serialize ServerMsg: {e}");
                    continue;
                }
            };
            if sender.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    });

    // Handle incoming messages from the client.
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => {
                match serde_json::from_str::<ClientMsg>(&text) {
                    Ok(ClientMsg::Ping) => {
                        // No-op for now, client can use WebSocket ping frames.
                    }
                    Ok(ClientMsg::SubscribeSchedule { date: _ }) => {
                        // No-op for now — future: filter broadcasts by date.
                    }
                    Err(e) => {
                        warn!("Invalid client message: {e}");
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    // Clean up: abort the send task when the client disconnects.
    send_task.abort();
    info!("WebSocket client disconnected");
}
