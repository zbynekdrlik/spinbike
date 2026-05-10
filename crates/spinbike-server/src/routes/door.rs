//! Door self-entry routes.
//!
//! `POST /api/door/open` — authenticated customer opens the studio door:
//!   * Pre-flight: role + `allow_self_entry` + per-user / global rate limits.
//!   * BEGIN tx → count today's `door:` rows → decide visit-or-charge for the
//!     first press, or zero-amount label for the Nth.
//!   * Call `state.ewelink.press()` (real cloud or test stub).
//!   * COMMIT on Ok, ROLLBACK + 503 on Err. The tx guarantees we never
//!     bill a customer for a press that didn't physically open the door.
//!
//! `GET /api/door/health` — admin/staff only, surfaces WS state + last ack age
//! for the operator dashboard.
//!
//! Rate limit: per-user 10 s between consecutive presses + 5/min/user hard cap,
//! plus a global 30/min cap across all users. State is in-memory (single-
//! instance server). The rate-limit press is recorded BEFORE the press call so
//! anti-abuse throttling still applies when the hardware errors.

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use crate::AppState;
use crate::auth::AuthUser;
use crate::ewelink::EwelinkState;
use crate::routes::internal_error;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/door/open", post(open))
        .route("/api/door/health", get(health))
}

// ---------- Rate limiter ----------

/// In-memory rate-limit state for the door route.
///
/// Single-instance server, so a per-process struct is enough — no Redis.
/// Stored as an `Arc<Mutex<RateLimiter>>` on `AppState` so concurrent
/// integration tests get their own throttle windows.
pub struct RateLimiter {
    per_user: HashMap<i64, VecDeque<Instant>>,
    global: VecDeque<Instant>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            per_user: HashMap::new(),
            global: VecDeque::new(),
        }
    }

    /// Returns Ok if this press is allowed and records it. Err with a short
    /// reason tag otherwise. Reason tags are logged but the HTTP response
    /// flattens them to "rate_limited" per spec.
    fn check_and_record(&mut self, user_id: i64) -> Result<(), &'static str> {
        let now = Instant::now();
        // Prune global window (60s).
        while let Some(front) = self.global.front() {
            if now.duration_since(*front) > Duration::from_secs(60) {
                self.global.pop_front();
            } else {
                break;
            }
        }
        if self.global.len() >= 30 {
            return Err("global_cap");
        }
        // Prune per-user window (60s).
        let q = self.per_user.entry(user_id).or_default();
        while let Some(front) = q.front() {
            if now.duration_since(*front) > Duration::from_secs(60) {
                q.pop_front();
            } else {
                break;
            }
        }
        if let Some(last) = q.back()
            && now.duration_since(*last) < Duration::from_secs(10)
        {
            return Err("too_fast");
        }
        if q.len() >= 5 {
            return Err("per_user_cap");
        }
        q.push_back(now);
        self.global.push_back(now);
        Ok(())
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------- POST /api/door/open ----------

async fn open(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let user_id = claims.sub;

    // 1. Load user + role + allow_self_entry + credit.
    let row: Option<(String, i64, f64)> = sqlx::query_as(
        "SELECT role, allow_self_entry, credit FROM users \
         WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?;

    let (role, allow_self_entry, mut credit) = match row {
        Some(r) => r,
        None => {
            tracing::warn!(user_id, "door: rejected — user not found or deleted");
            return Ok((
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"status": "rejected", "reason": "not_allowed"})),
            ));
        }
    };

    if role != "customer" {
        tracing::warn!(user_id, %role, "door: rejected — role is not customer");
        return Ok((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"status": "rejected", "reason": "not_allowed"})),
        ));
    }
    if allow_self_entry == 0 {
        tracing::warn!(user_id, "door: rejected — allow_self_entry is 0");
        return Ok((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"status": "rejected", "reason": "not_allowed"})),
        ));
    }

    // 2. Rate-limit check. Records the press attempt regardless of hardware
    // outcome — anti-abuse is throttled even when the cloud is down.
    if let Err(reason) = state
        .door_rate_limit
        .lock()
        .expect("rate-limiter mutex poisoned")
        .check_and_record(user_id)
    {
        tracing::warn!(user_id, %reason, "door: rejected — rate limited");
        return Ok((
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({"status": "rejected", "reason": "rate_limited"})),
        ));
    }

    // 3. BEGIN tx. Everything until press() and commit/rollback happens
    // inside this tx so we never write a billing row for a press that
    // didn't physically open the door.
    let mut tx = state.pool.begin().await.map_err(internal_error)?;

    // 4. Same-day count.
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM transactions \
         WHERE user_id = ? \
           AND note LIKE 'door:%' \
           AND date(created_at, 'localtime') = date('now', 'localtime') \
           AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal_error)?;

    let mut charged = false;
    let door_count_today = n + 1;

    // 5. Build the row to insert (action/service_id/amount/note).
    let (action, service_id_opt, amount, note): (&str, Option<i64>, f64, String) = if n == 0 {
        // First press today — visit or charge depending on monthly-pass state.
        let pass_active: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM transactions \
             WHERE user_id = ? \
               AND action = 'charge' \
               AND service_id = (SELECT id FROM services WHERE kind = 'monthly_pass') \
               AND valid_until > datetime('now') \
               AND deleted_at IS NULL \
             LIMIT 1",
        )
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal_error)?;

        if pass_active.is_some() {
            // Monthly pass covers the entry — zero-amount visit row.
            ("visit", None, 0.0, "door: 1st".to_string())
        } else {
            // No pass — charge single_entry price and deduct from user.credit.
            let svc: Option<(i64, f64)> = sqlx::query_as(
                "SELECT id, default_price FROM services \
                 WHERE kind = 'single_entry' AND active = 1 LIMIT 1",
            )
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal_error)?;
            let (svc_id, price) = match svc {
                Some(s) => s,
                None => {
                    tracing::error!("door: no active single_entry service seeded");
                    return Err(internal_error("no active single_entry service configured"));
                }
            };
            // Deduct from running balance.
            sqlx::query("UPDATE users SET credit = credit - ? WHERE id = ?")
                .bind(price)
                .bind(user_id)
                .execute(&mut *tx)
                .await
                .map_err(internal_error)?;
            credit -= price;
            charged = true;
            ("charge", Some(svc_id), -price, "door: 1st".to_string())
        }
    } else {
        // N-th press today (N >= 2) — zero-amount marker row.
        let ord = crate::util::ordinal((n + 1) as u32);
        ("charge", None, 0.0, format!("door: {ord}"))
    };

    // 6. Insert the row (still uncommitted).
    sqlx::query(
        "INSERT INTO transactions \
           (user_id, staff_id, service_id, amount, action, valid_until, note) \
         VALUES (?, NULL, ?, ?, ?, NULL, ?)",
    )
    .bind(user_id)
    .bind(service_id_opt)
    .bind(amount)
    .bind(action)
    .bind(&note)
    .execute(&mut *tx)
    .await
    .map_err(internal_error)?;

    // 7. Press the relay. ON FAILURE: drop the tx (auto-rollback) and 503.
    match state.ewelink.press().await {
        Ok(()) => {
            tx.commit().await.map_err(internal_error)?;
            tracing::info!(
                user_id,
                door_count_today,
                charged,
                new_credit = credit,
                %note,
                "door: opened"
            );
            Ok((
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "opened",
                    "reason": "ok",
                    "new_credit": credit,
                    "door_count_today": door_count_today,
                    "charged": charged,
                })),
            ))
        }
        Err(e) => {
            // Drop tx WITHOUT commit → SQLite rolls back.
            drop(tx);
            tracing::error!(user_id, err = %e, "door: hardware press failed, rolled back");
            Ok((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "status": "rejected",
                    "reason": "hardware_unavailable",
                })),
            ))
        }
    }
}

// ---------- GET /api/door/health ----------

/// Require admin OR staff. Mirrors `admin::require_staff` but inlined to keep
/// the door module self-contained.
fn require_admin_or_staff(
    claims: &spinbike_core::auth::Claims,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if matches!(
        claims.role,
        spinbike_core::auth::Role::Admin | spinbike_core::auth::Role::Staff
    ) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ))
    }
}

async fn health(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    require_admin_or_staff(&claims)?;

    let ws_state = match state.ewelink.state() {
        EwelinkState::Connected => "connected",
        EwelinkState::Disconnected => "disconnected",
        EwelinkState::Disabled => "disabled",
    };
    let last_ack_ms_ago = state.ewelink.last_ack_ms_ago();

    Ok(Json(serde_json::json!({
        "ewelink_ws": ws_state,
        "last_ack_ms_ago": last_ack_ms_ago,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limiter_allows_first_press() {
        let mut rl = RateLimiter::new();
        assert!(rl.check_and_record(1).is_ok());
    }

    #[test]
    fn rate_limiter_blocks_consecutive_within_10s() {
        let mut rl = RateLimiter::new();
        assert!(rl.check_and_record(1).is_ok());
        // Immediate second press → too_fast.
        assert_eq!(rl.check_and_record(1), Err("too_fast"));
    }

    #[test]
    fn rate_limiter_per_user_independent() {
        let mut rl = RateLimiter::new();
        assert!(rl.check_and_record(1).is_ok());
        // User 2's first press is independent.
        assert!(rl.check_and_record(2).is_ok());
    }
}
