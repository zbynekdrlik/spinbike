//! Error taxonomy for the ewelink module. Each variant maps to a specific
//! 503 / 500 path in the door route; matching on the variant in tracing
//! lets us know exactly what to fix when a press fails.

#[derive(Debug, thiserror::Error)]
pub enum EwelinkError {
    #[error("ewelink auth failed: {0}")]
    Auth(String),

    #[error("ewelink network error: {0}")]
    Network(String),

    #[error("device offline")]
    DeviceOffline,

    #[error("device ack timed out after 5s")]
    DeviceTimeout,

    #[error("bad response: {0}")]
    BadResponse(String),

    /// EWELINK_* env vars unset — module is in disabled mode. press() never
    /// reaches a network. Door route treats this the same as a 503 to the
    /// caller, but the log message distinguishes "not configured" from
    /// "configured but broken".
    #[error("ewelink module disabled (env vars unset)")]
    Disabled,
}
