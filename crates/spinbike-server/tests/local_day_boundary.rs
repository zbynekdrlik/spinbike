//! #222 — the remaining day-boundary sites keyed off the gym's LOCAL day
//! (Europe/Bratislava), not SQLite's UTC `date('now')` / OS-zone `'localtime'`.
//!
//! These DB-level tests exercise the EXACT predicates the handlers now run —
//! the door same-day re-entry count (`door.rs`) and the "my bookings" upcoming
//! filter (`db::classes::list_user_bookings`) — with a caller-supplied gym-local
//! "today" (as `util::today_bratislava()` / `util::bratislava_day_range_utc()`
//! supply in production). Using a FIXED date far from the wall clock makes them
//! deterministic on ANY CI-runner timezone (the runner is UTC), and pins the
//! failure mode: `created_at` is a UTC INSTANT, so a naive
//! `date(created_at,'localtime')` compare on a UTC host mis-attributes a press
//! made just after local midnight to the previous UTC day.

use chrono::NaiveDate;
use spinbike_server::db::{create_memory_pool, run_migrations};
use spinbike_server::util::bratislava_day_range_utc;

async fn seed_user(pool: &sqlx::SqlitePool) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO users (email, name, credit) VALUES ('u@x','u',0.0) RETURNING id",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Insert a `door:` transaction row at an explicit UTC `created_at` instant
/// (`service_id` is nullable and irrelevant to the same-day count, which keys
/// off `note LIKE 'door:%'`).
async fn seed_door(pool: &sqlx::SqlitePool, user_id: i64, created_at_utc: &str) {
    sqlx::query(
        "INSERT INTO transactions (user_id, service_id, amount, action, note, created_at) \
         VALUES (?, NULL, 0, 'visit', 'door: 1st', ?)",
    )
    .bind(user_id)
    .bind(created_at_utc)
    .execute(pool)
    .await
    .unwrap();
}

/// The EXACT same-day door count predicate `door.rs` runs: door rows whose UTC
/// `created_at` falls in the gym-local day's UTC-instant range.
async fn door_count_for_gym_day(
    pool: &sqlx::SqlitePool,
    user_id: i64,
    gym_today: NaiveDate,
) -> i64 {
    let (start, end) = bratislava_day_range_utc(gym_today);
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM transactions \
         WHERE user_id = ? AND note LIKE 'door:%' \
           AND created_at >= ? AND created_at < ? AND deleted_at IS NULL",
    )
    .bind(user_id)
    .bind(start.format("%Y-%m-%d %H:%M:%S").to_string())
    .bind(end.format("%Y-%m-%d %H:%M:%S").to_string())
    .fetch_one(pool)
    .await
    .unwrap()
}

/// WINTER (CET, UTC+1): a press at 00:30 Bratislava on 2026-01-16 is stored as
/// 23:30 UTC on 2026-01-15 — a DIFFERENT UTC calendar day. It must count on the
/// gym day 2026-01-16, NOT 2026-01-15. Under the old
/// `date(created_at,'localtime')` compare on a UTC host it was mis-dated to the
/// 15th — the exact same-day-count bug this fixes.
#[tokio::test]
async fn door_press_just_after_local_midnight_counts_on_the_gym_day_winter() {
    let pool = create_memory_pool().await.unwrap();
    run_migrations(&pool).await.unwrap();
    let uid = seed_user(&pool).await;
    // 2026-01-15 23:30 UTC == 2026-01-16 00:30 CET (gym).
    seed_door(&pool, uid, "2026-01-15 23:30:00").await;

    assert_eq!(
        door_count_for_gym_day(&pool, uid, NaiveDate::from_ymd_opt(2026, 1, 16).unwrap()).await,
        1,
        "a press at 00:30 gym-time belongs to gym day 2026-01-16"
    );
    assert_eq!(
        door_count_for_gym_day(&pool, uid, NaiveDate::from_ymd_opt(2026, 1, 15).unwrap()).await,
        0,
        "it must NOT count on the UTC calendar day (2026-01-15) — that's the OS-TZ bug"
    );
}

/// SUMMER (CEST, UTC+2): 22:30 UTC on 2026-07-15 is 00:30 the next day at the
/// gym (2026-07-16). Pins the live DST offset — a hardcoded +01:00 would place
/// it at 23:30 local, still the 15th, and mis-count.
#[tokio::test]
async fn door_press_just_after_local_midnight_counts_on_the_gym_day_summer() {
    let pool = create_memory_pool().await.unwrap();
    run_migrations(&pool).await.unwrap();
    let uid = seed_user(&pool).await;
    // 2026-07-15 22:30 UTC == 2026-07-16 00:30 CEST (gym).
    seed_door(&pool, uid, "2026-07-15 22:30:00").await;

    assert_eq!(
        door_count_for_gym_day(&pool, uid, NaiveDate::from_ymd_opt(2026, 7, 16).unwrap()).await,
        1
    );
    assert_eq!(
        door_count_for_gym_day(&pool, uid, NaiveDate::from_ymd_opt(2026, 7, 15).unwrap()).await,
        0
    );
}

/// Money-adjacency (double-charge guard): two presses on the SAME gym day but
/// on DIFFERENT UTC calendar days (split by UTC midnight, which is 01:00 gym
/// time in winter) must count TOGETHER on that gym day — so the 2nd press sees
/// `n >= 1`, is labelled "2nd", and does NOT re-run the first-of-day charge
/// path. The old UTC-day compare would see two separate "1st"s → a second
/// single-entry charge for one gym day.
#[tokio::test]
async fn two_presses_same_gym_day_across_utc_midnight_count_together() {
    let pool = create_memory_pool().await.unwrap();
    run_migrations(&pool).await.unwrap();
    let uid = seed_user(&pool).await;
    // gym day 2026-01-16: 00:30 gym (2026-01-15 23:30 UTC) + 10:00 gym
    // (2026-01-16 09:00 UTC) — DIFFERENT UTC days, SAME gym day.
    seed_door(&pool, uid, "2026-01-15 23:30:00").await;
    seed_door(&pool, uid, "2026-01-16 09:00:00").await;

    assert_eq!(
        door_count_for_gym_day(&pool, uid, NaiveDate::from_ymd_opt(2026, 1, 16).unwrap()).await,
        2,
        "both presses are on gym day 2026-01-16 → counted together (2nd is not a fresh 1st)"
    );
}

/// The other direction: two presses on the SAME UTC day but DIFFERENT gym days
/// (one just before, one just after local midnight) must each be the "1st" of
/// its OWN gym day — the old compare merged them into one UTC day, hiding the
/// second gym day's first entry (skipping its pass check / charge).
#[tokio::test]
async fn two_presses_split_by_local_midnight_count_on_their_own_gym_days() {
    let pool = create_memory_pool().await.unwrap();
    run_migrations(&pool).await.unwrap();
    let uid = seed_user(&pool).await;
    // 2026-01-15 21:30 UTC == 2026-01-15 22:30 CET (gym day 15).
    seed_door(&pool, uid, "2026-01-15 21:30:00").await;
    // 2026-01-15 23:30 UTC == 2026-01-16 00:30 CET (gym day 16).
    seed_door(&pool, uid, "2026-01-15 23:30:00").await;

    // Same UTC calendar day (both 2026-01-15 UTC) but different gym days.
    assert_eq!(
        door_count_for_gym_day(&pool, uid, NaiveDate::from_ymd_opt(2026, 1, 15).unwrap()).await,
        1
    );
    assert_eq!(
        door_count_for_gym_day(&pool, uid, NaiveDate::from_ymd_opt(2026, 1, 16).unwrap()).await,
        1
    );
}

/// "My bookings" upcoming filter (`list_user_bookings`) — replicated exactly:
/// `b.date >= ?` where `?` is the gym-local "today" (`today_bratislava()`), a
/// bare-date compare bound as a PARAMETER, not SQLite `date('now')`. A booking
/// dated the bound gym-today appears (inclusive); one dated the day before is
/// excluded. FIXED 2020 dates prove the filter honors the parameter, not
/// `date('now')` (under which a 2020 "today" could never bound anything).
#[tokio::test]
async fn my_bookings_filter_honors_gym_local_today_not_date_now() {
    let pool = create_memory_pool().await.unwrap();
    run_migrations(&pool).await.unwrap();
    let uid = seed_user(&pool).await;
    let tid: i64 = sqlx::query_scalar("SELECT id FROM class_templates LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    for d in ["2020-06-14", "2020-06-15", "2020-06-16"] {
        sqlx::query(
            "INSERT INTO bookings (template_id, date, user_id, source) VALUES (?, ?, ?, 'manual')",
        )
        .bind(tid)
        .bind(d)
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    }

    let today = NaiveDate::from_ymd_opt(2020, 6, 15).unwrap();
    let got: Vec<String> = sqlx::query_scalar(
        "SELECT b.date FROM bookings b \
          WHERE b.user_id = ? AND b.cancelled_at IS NULL AND b.date >= ? \
          ORDER BY b.date",
    )
    .bind(uid)
    .bind(today)
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(
        got,
        vec!["2020-06-15".to_string(), "2020-06-16".to_string()],
        "the list starts at the bound gym-local today (inclusive), dropping earlier days — \
         proving `b.date >= ?` honors the parameter, not SQLite date('now')"
    );
}
