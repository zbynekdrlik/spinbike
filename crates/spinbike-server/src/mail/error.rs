//! Error taxonomy for the mail module. `Disabled` is what route handlers
//! (added in a later ticket) map to a 503 `mail_not_configured` response;
//! `Send` and `Timeout` are runtime failures once mail IS configured.

#[derive(Debug, thiserror::Error)]
pub enum MailError {
    /// Required SMTP_* env vars are unset/empty (or `SMTP_PORT` doesn't
    /// parse), so no transport was built — `send()` never reaches the
    /// network. Distinguishes "not configured" from "configured but
    /// broken" in logs and in the 503 the caller returns.
    #[error("mail module disabled (SMTP_* env vars unset)")]
    Disabled,

    /// Message composition or the SMTP exchange failed. The inner string
    /// carries the underlying lettre error for logs; it is built from
    /// error `Display` output and never includes SMTP_PASSWORD.
    #[error("mail send failed: {0}")]
    Send(String),

    /// The SMTP exchange did not complete within the 10 s send timeout.
    #[error("mail send timed out after 10s")]
    Timeout,
}
