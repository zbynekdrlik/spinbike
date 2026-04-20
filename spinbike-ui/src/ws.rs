use futures::StreamExt;
use gloo_net::websocket::{Message, futures::WebSocket};
use leptos::prelude::*;
use spinbike_core::ws::ServerMsg;
use wasm_bindgen_futures::spawn_local;

/// Provides a reactive signal that emits the latest ServerMsg from the WebSocket.
pub fn connect_ws() -> ReadSignal<Option<ServerMsg>> {
    let (read, write) = signal(None::<ServerMsg>);

    spawn_local(async move {
        ws_loop(write).await;
    });

    read
}

async fn ws_loop(set_msg: WriteSignal<Option<ServerMsg>>) {
    let mut delay_ms: u32 = 1000;
    let max_delay: u32 = 30_000;

    loop {
        let ws_url = build_ws_url();
        match WebSocket::open(&ws_url) {
            Ok(ws) => {
                delay_ms = 1000;
                let (_write, mut read) = ws.split();

                while let Some(msg) = read.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
                            if let Ok(server_msg) = serde_json::from_str::<ServerMsg>(&text) {
                                set_msg.set(Some(server_msg));
                            }
                        }
                        Ok(Message::Bytes(_)) => {}
                        Err(_) => break,
                    }
                }
            }
            Err(_) => {}
        }

        gloo_timers::future::sleep(std::time::Duration::from_millis(u64::from(delay_ms))).await;
        delay_ms = (delay_ms * 2).min(max_delay);
    }
}

fn build_ws_url() -> String {
    let window = web_sys::window().expect("no window");
    let location = window.location();
    let protocol = location.protocol().unwrap_or_else(|_| "http:".into());
    let host = location.host().unwrap_or_else(|_| "localhost".into());
    let ws_proto = if protocol == "https:" { "wss:" } else { "ws:" };
    format!("{ws_proto}//{host}/api/ws")
}
