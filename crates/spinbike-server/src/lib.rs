pub mod db;

use spinbike_core::ws::ServerMsg;
use sqlx::SqlitePool;
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub event_tx: broadcast::Sender<ServerMsg>,
    pub jwt_secret: String,
}
