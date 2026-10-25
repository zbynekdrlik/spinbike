pub mod auth;
pub mod booking;
pub mod errors;
pub mod models;
pub mod redact;
pub mod reports;
pub mod services;
pub mod stats;
pub mod ws;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
