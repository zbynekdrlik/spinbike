//! Integration test for the eWeLink WS dispatch loop.
//! Spins up a tokio-tungstenite SERVER mocking the eWeLink dispatcher;
//! the real client connects to it via plain ws://, sends a press, and
//! we assert the round-trip works.

use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU8};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::{accept_async, tungstenite::Message};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mock_ws_round_trip() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let ws_url = format!("ws://{addr}/dispatch/app");

    // Mock server task
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();

        // Wait for userOnline → reply error:0
        let msg = ws.next().await.unwrap().unwrap();
        assert!(msg.to_text().unwrap().contains("\"action\":\"userOnline\""));
        ws.send(Message::Text(r#"{"error":0,"apikey":"k"}"#.into()))
            .await
            .unwrap();

        // Wait for update press → reply error:0 with same sequence
        let msg = ws.next().await.unwrap().unwrap();
        let text = msg.to_text().unwrap().to_string();
        assert!(text.contains("\"action\":\"update\""));
        assert!(text.contains("\"switch\":\"on\""));
        let seq = text
            .split("\"sequence\":\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .to_string();
        ws.send(Message::Text(format!(
            r#"{{"error":0,"sequence":"{seq}"}}"#
        )))
        .await
        .unwrap();
    });

    // Client side: spawn connect_loop_with_url pointed at the mock
    let (tx, rx) = mpsc::channel::<spinbike_server::ewelink::PressRequest>(8);
    let state = Arc::new(AtomicU8::new(0));
    let last_ack_ms = Arc::new(AtomicI64::new(i64::MIN));

    let _client_task = tokio::spawn({
        let state = state.clone();
        let last_ack_ms = last_ack_ms.clone();
        async move {
            spinbike_server::ewelink::ws::connect_loop_with_url(
                &ws_url,
                "<unused>",
                "<unused-apikey>",
                "test-device",
                state,
                last_ack_ms,
                rx,
            )
            .await;
        }
    });

    // Give the client a moment to connect + handshake
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Send a press
    let (ack_tx, ack_rx) = oneshot::channel();
    tx.send(spinbike_server::ewelink::PressRequest { ack: ack_tx })
        .await
        .unwrap();

    let result = tokio::time::timeout(Duration::from_secs(5), ack_rx)
        .await
        .expect("press did not ack within 5s")
        .expect("oneshot dropped");
    assert!(result.is_ok(), "press should succeed, got {result:?}");

    server.await.unwrap();
}
