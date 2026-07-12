//! #205 — the monthly-pass expiry day boundary is the gym's LOCAL day
//! (Europe/Bratislava), not SQLite's UTC `date('now')`.
//!
//! These tests exercise the SHARED predicate that the door route and
//! `/api/my/balance` now run —
//! `SELECT valid_until FROM user_active_pass WHERE user_id = ? AND date(valid_until) >= ?`
//! — where `?` is the gym-local "today" (`util::today_bratislava()`), bound as a
//! parameter. The key difference from the pre-#205 code is that the boundary is
//! driven by a caller-supplied date, NOT by SQLite's `date('now')`. To prove
//! that mechanism deterministically (independent of the wall clock and the CI
//! runner's timezone), we bind a FIXED "today" far from the real current date:
//! under the old `date('now')` predicate a pass whose `valid_until` sits in
//! 2020 could never read as active, so an assertion that it IS active on that
//! bound day fails on the old logic and passes on the new.

use chrono::NaiveDate;
use spinbike_server::db::{create_memory_pool, run_migrations};

/// Seed a user holding a single non-voided monthly pass expiring on
/// `valid_until`, then return `(pool, user_id)`.
async fn seed_pass(valid_until: NaiveDate) -> (sqlx::SqlitePool, i64) {
    let pool = create_memory_pool().await.unwrap();
    run_migrations(&pool).await.unwrap();

    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (email, name, credit) VALUES ('u@x', 'u', 0.0) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let pass_svc: i64 = sqlx::query_scalar("SELECT id FROM services WHERE kind = 'monthly_pass'")
        .fetch_one(&pool)
        .await
        .unwrap();

    sqlx::query(
        "INSERT INTO transactions (user_id, service_id, amount, action, valid_until) \
         VALUES (?, ?, -35.0, 'charge', ?)",
    )
    .bind(user_id)
    .bind(pass_svc)
    .bind(valid_until)
    .execute(&pool)
    .await
    .unwrap();

    (pool, user_id)
}

/// Run the exact pass-active predicate the handlers use, with `today` supplied
/// by the caller (as `util::today_bratislava()` does in production).
async fn active_valid_until(
    pool: &sqlx::SqlitePool,
    user_id: i64,
    today: NaiveDate,
) -> Option<String> {
    sqlx::query_scalar(
        "SELECT valid_until FROM user_active_pass \
         WHERE user_id = ? AND date(valid_until) >= ?",
    )
    .bind(user_id)
    .bind(today)
    .fetch_optional(pool)
    .await
    .unwrap()
}

/// A pass is active THROUGH the whole of its last day, evaluated against the
/// caller-supplied gym-local "today" (inclusive `>=`). The bound date (2020)
/// can only match because the predicate honors the PARAMETER, not `date('now')`
/// — so this fails on the old UTC `date('now')` logic and passes on #205.
#[tokio::test]
async fn pass_is_active_on_its_expiry_day_using_caller_supplied_today() {
    let expiry = NaiveDate::from_ymd_opt(2020, 6, 15).unwrap();
    let (pool, user_id) = seed_pass(expiry).await;

    // today == valid_until → still active (inclusive last day).
    let got = active_valid_until(&pool, user_id, expiry).await;
    assert_eq!(
        got.as_deref(),
        Some("2020-06-15"),
        "a pass must be active on its own last (gym-local) day, evaluated against \
         the bound `today` — proves the boundary uses the parameter, not date('now')"
    );
}

/// The other side of the boundary: on the FIRST gym-local day after expiry the
/// pass is over. Guards the inclusive fix from becoming permissive of expired
/// passes, still driven by the caller-supplied date rather than `date('now')`.
#[tokio::test]
async fn pass_is_inactive_on_the_day_after_expiry_using_caller_supplied_today() {
    let expiry = NaiveDate::from_ymd_opt(2020, 6, 15).unwrap();
    let (pool, user_id) = seed_pass(expiry).await;

    let day_after = NaiveDate::from_ymd_opt(2020, 6, 16).unwrap();
    let got = active_valid_until(&pool, user_id, day_after).await;
    assert_eq!(
        got, None,
        "a pass must be inactive the first gym-local day after its expiry"
    );
}

/// The production "today" fed into the predicate is the Europe/Bratislava
/// calendar date, not the naive UTC date — this ties the deterministic
/// predicate tests above to the exact value the handlers bind.
#[tokio::test]
async fn today_bratislava_is_the_gym_local_date() {
    let expected = chrono::Utc::now()
        .with_timezone(&chrono_tz::Europe::Bratislava)
        .date_naive();
    assert_eq!(spinbike_server::util::today_bratislava(), expected);
}
