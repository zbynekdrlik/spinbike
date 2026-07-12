//! Door self-entry routes.
//!
//! `POST /api/door/open` — authenticated customer opens the studio door:
//!   * Pre-flight: `blocked` (every role, no bypass) + role + `allow_self_entry`
//!     + per-user / global rate limits.
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
use std::time::{Duration, Instant};

use crate::AppState;
use crate::auth::{AuthUser, StaffUser};
use crate::error::ApiError;
use crate::ewelink::EwelinkState;
use crate::rate_limit::{RateLimitConfig, SlidingWindowLimiter};
use crate::routes::internal_error;
use spinbike_core::auth::Role;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/door/open", post(open))
        .route("/api/door/health", get(health))
}

// ---------- Rate limiter ----------

/// In-memory rate-limit state for the door route: per-user 10 s min-gap + a
/// 5/60 s per-user cap under a global 30/60 s cap. A thin typed wrapper over the
/// shared `SlidingWindowLimiter` (#166) — one abstraction backs both this and
/// the login-link limiter; the per-user map now evicts quiet keys (the leak the
/// old hand-rolled copy carried).
///
/// Stored as an `Arc<Mutex<RateLimiter>>` on `AppState` so concurrent
/// integration tests get their own throttle windows.
pub struct RateLimiter(SlidingWindowLimiter<i64>);

impl RateLimiter {
    pub fn new() -> Self {
        Self(SlidingWindowLimiter::new(RateLimitConfig {
            per_key_window: Duration::from_secs(60),
            per_key_min_gap: Some(Duration::from_secs(10)),
            per_key_max: Some(5),
            per_key_cap_reason: "per_user_cap",
            key_memory: Duration::from_secs(60),
            global_window: Duration::from_secs(60),
            global_max: 30,
        }))
    }

    /// Returns Ok if this press is allowed and records it. Err with a short
    /// reason tag ("too_fast" / "per_user_cap" / "global_cap") otherwise; the
    /// HTTP response flattens it to "rate_limited" per spec.
    pub fn check_and_record(&mut self, user_id: i64) -> Result<(), &'static str> {
        self.0.check_and_record(user_id)
    }

    /// Same as `check_and_record` but takes the current `Instant` so unit tests
    /// can simulate elapsed time without sleeping.
    pub fn check_and_record_at(&mut self, user_id: i64, now: Instant) -> Result<(), &'static str> {
        self.0.check_and_record_at(user_id, now)
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
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let user_id = claims.sub;

    // 1. Load user + role + allow_self_entry + credit + blocked.
    let row: Option<(String, i64, f64, bool)> = sqlx::query_as(
        "SELECT role, allow_self_entry, credit, blocked FROM users \
         WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?;

    let (role, allow_self_entry, mut credit, blocked) = match row {
        Some(r) => r,
        None => {
            tracing::warn!(user_id, "door: rejected — user not found or deleted");
            return Ok((
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"status": "rejected", "reason": "not_allowed"})),
            ));
        }
    };

    // Blocked-means-blocked for every role, including admin/staff — a
    // blocked staff account must not be able to actuate hardware. Checked
    // BEFORE the allow_self_entry role bypass below so it can never be
    // skipped by that bypass (#106).
    //
    // Reason tag is "blocked" (not the `{"error": "User is blocked"}` shape
    // used by payments.rs/users.rs) — intentional: this route's own envelope
    // is already `{"status":"rejected","reason":"<tag>"}` (see "not_allowed"
    // and "rate_limited" below), so this stays consistent with the OTHER
    // rejections in this same file rather than mixing two response shapes.
    if blocked {
        tracing::warn!(user_id, %role, "door: rejected — user is blocked");
        return Ok((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"status": "rejected", "reason": "blocked"})),
        ));
    }

    // Admin/staff bypass the allow_self_entry gate — they run the place,
    // they don't need their own opt-in toggle. Customers still need the CEO
    // to enable the flag. Billing logic below: admin/staff always log a
    // visit (no charge); customers follow pass / charge / Nth-of-day flow.
    let is_staff_or_admin_role = Role::from(role.as_str()).is_staff_or_admin();
    if !is_staff_or_admin_role && allow_self_entry == 0 {
        tracing::warn!(
            user_id,
            %role,
            "door: rejected — allow_self_entry is 0"
        );
        return Ok((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"status": "rejected", "reason": "not_allowed"})),
        ));
    }

    // 2. Rate-limit check. Records the press attempt regardless of hardware
    // outcome — anti-abuse is throttled even when the cloud is down.
    if let Err(reason) = state
        .door_rate_limit
        // #172: panic="unwind" (was "abort") means a future panic while this
        // guard is held now actually poisons the mutex instead of aborting
        // the whole process. Recover the guard rather than propagate the
        // poison via .expect() — a sliding-window hit counter has no
        // invariant that a mid-update panic could leave inconsistent enough
        // to justify permanently 500-ing every door request until restart.
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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

    // 4. Same-day count — how many door rows this user already has TODAY, used
    // to label the press 1st/2nd/Nth AND (money-adjacent) to pick the path
    // below: `n == 0` is the first press of the day and triggers the
    // pass-check-or-charge branch. "Today" is the gym's LOCAL day
    // (Europe/Bratislava). `created_at` is a UTC INSTANT (`datetime('now')`), so
    // we compare it against the UTC-instant half-open RANGE of the gym day —
    // NOT `date(created_at,'localtime') = date('now','localtime')`, whose
    // 'localtime' reads the server OS zone. On a UTC-configured host that old
    // form counts by the UTC day and, near local midnight, could split two
    // same-gym-day presses across a stale rollover (making the 2nd look like a
    // fresh 1st → a second charge) or merge two different gym days. The bound
    // range makes the boundary exact and DST-correct (#205/#222).
    let (day_start, day_end) =
        crate::util::bratislava_day_range_utc(crate::util::today_bratislava());
    let day_start = day_start.format("%Y-%m-%d %H:%M:%S").to_string();
    let day_end = day_end.format("%Y-%m-%d %H:%M:%S").to_string();
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM transactions \
         WHERE user_id = ? \
           AND note LIKE 'door:%' \
           AND created_at >= ? \
           AND created_at < ? \
           AND deleted_at IS NULL",
    )
    .bind(user_id)
    .bind(&day_start)
    .bind(&day_end)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal_error)?;

    let mut charged = false;
    let door_count_today = n + 1;

    // Look up the single_entry service id ONCE — used for ALL door tx rows
    // (1st-with-pass visit, 1st-no-pass charge, Nth-of-day audit row). This
    // is so the existing attendance KPI report SQL (which filters by
    // service_id IN (single_entry, monthly_pass)) picks up door visits;
    // without it, door entries are invisible to reports.
    let single_entry_svc: Option<(i64, f64)> = sqlx::query_as(
        "SELECT id, default_price FROM services \
         WHERE kind = 'single_entry' AND active = 1 LIMIT 1",
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal_error)?;
    let (single_entry_id, single_entry_price) = match single_entry_svc {
        Some(s) => s,
        None => {
            tracing::error!("door: no active single_entry service seeded");
            return Err(internal_error("no active single_entry service configured"));
        }
    };

    // 5. Build the row to insert (action/service_id/amount/note).
    let (action, service_id_opt, amount, note): (&str, Option<i64>, f64, String) = if n == 0 {
        // First press today — visit or charge depending on monthly-pass state.
        // Admin/staff are never charged for their own door use — always
        // logged as a visit. Customer flow falls through to the pass check.
        let pass_active: Option<i64> = if is_staff_or_admin_role {
            Some(1) // short-circuit — treat as "pass covers it"
        } else {
            // Route the "does this user hold an active monthly pass?" check
            // through the canonical `user_active_pass` view (migration V18) —
            // the SAME single definition the T-4h charger, my_balance and the
            // staff user lists use — instead of a 7th hand-rolled copy of the
            // predicate (#159 unified the other six; #179 finishes the door).
            // The view already applies `action='charge' AND
            // service kind='monthly_pass' AND deleted_at IS NULL` and picks the
            // latest non-voided pass, so this site inherits the voided-pass fix
            // for free. Inclusive last-day semantics mirror the charger EXACTLY:
            // `date(valid_until) >= ?` coerces the (bare-date) valid_until and
            // compares it against today's GYM-LOCAL date, so a pass covers the
            // WHOLE of its last paid day. `valid_until > datetime('now')` (pre
            // #179) read the expiry day as already-expired via SQLite's
            // byte-wise TEXT ordering — charging the customer on a day their
            // pass still covered. The day boundary is now the gym's local
            // midnight (Europe/Bratislava) via `util::today_bratislava()`, bound
            // as a parameter — NOT SQLite's UTC `date('now')`, which near local
            // midnight is up to 2h off from the gym's day (#205).
            let today = crate::util::today_bratislava();
            sqlx::query_scalar(
                "SELECT 1 FROM user_active_pass \
                 WHERE user_id = ? \
                   AND date(valid_until) >= ? \
                 LIMIT 1",
            )
            .bind(user_id)
            .bind(today)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal_error)?
        };

        if pass_active.is_some() {
            // Monthly pass covers the entry — zero-amount visit row. Tag
            // with single_entry service_id so attendance reports count it.
            ("visit", Some(single_entry_id), 0.0, "door: 1st".to_string())
        } else {
            // No pass — charge single_entry price and deduct from user.credit.
            sqlx::query("UPDATE users SET credit = credit - ? WHERE id = ?")
                .bind(single_entry_price)
                .bind(user_id)
                .execute(&mut *tx)
                .await
                .map_err(internal_error)?;
            credit -= single_entry_price;
            charged = true;
            (
                "charge",
                Some(single_entry_id),
                -single_entry_price,
                "door: 1st".to_string(),
            )
        }
    } else {
        // N-th press today (N >= 2) — zero-amount audit row. Still tagged
        // with single_entry service_id so the row visually groups under the
        // same service in the user's tx history; the visit-definition memo
        // (`action='visit' OR (action='charge' AND amount<0 AND valid_until
        // IS NULL)`) excludes amount=0 charges from the visit count, so
        // reports do NOT double-count these.
        let ord = crate::util::ordinal((n + 1) as u32);
        ("charge", Some(single_entry_id), 0.0, format!("door: {ord}"))
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

async fn health(
    State(state): State<AppState>,
    _: StaffUser,
) -> Result<Json<serde_json::Value>, ApiError> {
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

    // ─── 10-second consecutive-press boundary ────────────────────────────────
    //
    // The check is `now.duration_since(*last) < Duration::from_secs(10)`.
    // Mutations to test: < → <= (would block at exactly 10 s) and < → ==
    // (would let almost any value through).

    /// At exactly 10 s of gap, the second press MUST be allowed.
    /// Catches the `<` → `<=` mutation on the consecutive-press check.
    #[test]
    fn rate_limiter_allows_at_exactly_10s_boundary() {
        let mut rl = RateLimiter::new();
        let t0 = Instant::now();
        rl.check_and_record_at(1, t0).unwrap();
        let result = rl.check_and_record_at(1, t0 + Duration::from_secs(10));
        assert!(
            result.is_ok(),
            "press 10s after the previous one MUST be allowed, got {result:?}"
        );
    }

    /// Just under 10 s (9.999 s) MUST still be too_fast.
    /// Catches the `<` → `<=` mutation AND `<` → `==`.
    #[test]
    fn rate_limiter_blocks_just_under_10s() {
        let mut rl = RateLimiter::new();
        let t0 = Instant::now();
        rl.check_and_record_at(1, t0).unwrap();
        let result = rl.check_and_record_at(1, t0 + Duration::from_millis(9_999));
        assert_eq!(result, Err("too_fast"));
    }

    /// 11 s gap is allowed.
    #[test]
    fn rate_limiter_allows_after_11s_gap() {
        let mut rl = RateLimiter::new();
        let t0 = Instant::now();
        rl.check_and_record_at(1, t0).unwrap();
        rl.check_and_record_at(1, t0 + Duration::from_secs(11))
            .expect("11s after the previous press must succeed");
    }

    // ─── per-user 5 / 60 s cap ──────────────────────────────────────────────
    //
    // The check is `if q.len() >= 5`. Mutants to test: >= → > (lets 6 in)
    // and >= → == (lets 7+ in but blocks at exactly 5).

    /// Exactly 5 presses spaced 11 s apart are allowed; the 6th is capped.
    /// Catches `>=` → `>` and `>=` → `==`.
    #[test]
    fn rate_limiter_per_user_cap_kicks_in_at_6th_press() {
        let mut rl = RateLimiter::new();
        let t0 = Instant::now();
        for i in 0..5 {
            let when = t0 + Duration::from_secs(11 * i as u64);
            rl.check_and_record_at(7, when)
                .unwrap_or_else(|e| panic!("press #{i} should succeed, got {e}"));
        }
        // 6th attempt — still within the 60 s sliding window (11 s × 5 = 55 s).
        let sixth_at = t0 + Duration::from_secs(11 * 5);
        let result = rl.check_and_record_at(7, sixth_at);
        assert_eq!(
            result,
            Err("per_user_cap"),
            "6th press inside the 60s window must hit per_user_cap"
        );
    }

    // ─── global 30 / 60 s cap ───────────────────────────────────────────────
    //
    // The check is `if self.global.len() >= 30`. Test:
    //   `>=` → `>` (lets 31 in)
    //   `>=` → `==` (lets 31+ in but blocks at exactly 30 — same effect for
    //                 this test boundary).

    /// 30 distinct users each get one press; the 31st distinct user is
    /// blocked with global_cap. Catches `>=` → `>` on the global cap.
    #[test]
    fn rate_limiter_global_cap_kicks_in_at_31st_press() {
        let mut rl = RateLimiter::new();
        let t0 = Instant::now();
        for uid in 1..=30 {
            // Stagger by 1ms so the 60s window still contains them all.
            let when = t0 + Duration::from_millis(uid as u64);
            rl.check_and_record_at(uid, when)
                .unwrap_or_else(|e| panic!("user {uid} should succeed, got {e}"));
        }
        let result = rl.check_and_record_at(31, t0 + Duration::from_millis(100));
        assert_eq!(
            result,
            Err("global_cap"),
            "31st distinct user inside the 60s window must hit global_cap"
        );
    }

    /// After the 60 s window slides past, an old global entry is pruned and
    /// the global counter goes back below the cap. This locks down the
    /// global window's `> Duration::from_secs(60)` prune condition; if
    /// that were mutated to `>=`, the prune logic would also fire at
    /// exactly 60 s (still safe), but if mutated to `<` the queue would
    /// never prune and we'd never recover.
    #[test]
    fn rate_limiter_global_window_prunes_after_60s() {
        let mut rl = RateLimiter::new();
        let t0 = Instant::now();
        for uid in 1..=30 {
            let when = t0 + Duration::from_millis(uid as u64);
            rl.check_and_record_at(uid, when).unwrap();
        }
        // 31st (any user) at t0 + 90 s — every prior entry is older than 60 s
        // and should have been pruned, leaving room for this press.
        let result = rl.check_and_record_at(99, t0 + Duration::from_secs(90));
        assert!(
            result.is_ok(),
            "global cap must clear after the 60s window slides; got {result:?}"
        );
    }

    /// Global prune at exactly 60 s — `>` strict means the entry is KEPT
    /// (elapsed of 60 s is NOT greater than 60 s). Catches mutation of `>`
    /// to `>=` on the global prune comparison.
    #[test]
    fn rate_limiter_global_keeps_entry_at_exactly_60s() {
        let mut rl = RateLimiter::new();
        let t0 = Instant::now();
        for uid in 1..=30 {
            rl.check_and_record_at(uid, t0).unwrap();
        }
        // At t0 + exactly 60 s, prune compares `Duration::from_secs(60) > 60s`
        // which is false under `>` (entries kept) but true under `>=` (pruned).
        // With entries still in the deque, 31st press hits global cap.
        let result = rl.check_and_record_at(99, t0 + Duration::from_secs(60));
        assert_eq!(
            result,
            Err("global_cap"),
            "at exactly 60 s, the > strict comparison must keep entries in the global window"
        );
    }

    /// Per-user prune at exactly 60 s — strict `>` keeps the oldest entry,
    /// so the user is still at cap. Catches `>` → `>=` and `>` → `==` on the
    /// per-user prune.
    #[test]
    fn rate_limiter_per_user_keeps_entry_at_exactly_60s() {
        let mut rl = RateLimiter::new();
        let t0 = Instant::now();
        // 5 entries 11 s apart so the 10 s consecutive check passes.
        for i in 0..5 {
            rl.check_and_record_at(1, t0 + Duration::from_secs(11 * i as u64))
                .unwrap();
        }
        // At t0 + 60 s, the first entry (at t0) is exactly 60 s old. Under `>`
        // strict it stays in the deque → len = 5 → 6th press hits per_user_cap.
        // Under `>=` mutation, it would be pruned → len = 4 → success.
        // Under `==` mutation, only entries EXACTLY 60 s old are pruned —
        // first entry pruned, others kept → len = 4 → also success.
        let result = rl.check_and_record_at(1, t0 + Duration::from_secs(60));
        assert_eq!(
            result,
            Err("per_user_cap"),
            "at exactly 60 s the per-user prune `>` must keep the oldest entry"
        );
    }

    /// User 1 is at the per-user cap; user 2 is still allowed independently.
    #[test]
    fn rate_limiter_per_user_isolation_under_caps() {
        let mut rl = RateLimiter::new();
        let t0 = Instant::now();
        for i in 0..5 {
            rl.check_and_record_at(1, t0 + Duration::from_secs(11 * i as u64))
                .unwrap();
        }
        // User 1 hits the cap.
        assert_eq!(
            rl.check_and_record_at(1, t0 + Duration::from_secs(55)),
            Err("per_user_cap")
        );
        // User 2 (first press for this user) is fine.
        rl.check_and_record_at(2, t0 + Duration::from_secs(55))
            .expect("independent user must not be affected by user 1's cap");
    }

    // The role guard for /api/door/health now lives in the `StaffUser`
    // extractor (#160). Its Admin/Staff-allow, Customer/Unknown-reject logic is
    // unit-tested on `Role::is_staff_or_admin()` in `spinbike-core::auth` and
    // end-to-end in `tests/door_route.rs` (`door_health_403_for_customer`).
}
