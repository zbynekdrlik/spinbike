//! Integration tests for the door-health fault-alert episode logic (#355).
//!
//! Drives `jobs::door_health::DoorHealthMonitor` directly with a plain
//! `bool` verdict (the same value `EwelinkHandle::is_faulty()` feeds it in
//! production) against a real migrated in-memory DB + a capture-mode
//! `MailHandle`, so the anti-spam dedup can be asserted end-to-end without
//! any WS/hardware handle:
//!   * a fault episode e-mails the owner exactly ONCE (repeated faulty
//!     checks do not re-send),
//!   * recovery e-mails exactly ONCE,
//!   * a failed send is NOT deduped — it retries on the next tick,
//!   * the alert reaches every admin recipient.

use spinbike_server::db::{self, users};
use spinbike_server::jobs::door_health::{DoorHealthMonitor, DoorHealthTick};
use spinbike_server::mail::MailHandle;
use spinbike_server::util;

/// Fresh migrated in-memory DB (no users seeded — base migrations create
/// none without legacy data).
async fn empty_pool() -> sqlx::SqlitePool {
    let pool = db::create_memory_pool().await.unwrap();
    db::run_migrations(&pool).await.unwrap();
    pool
}

async fn seed_admin(pool: &sqlx::SqlitePool, email: &str) {
    users::create_user(
        pool,
        Some(email),
        None,
        "Owner",
        None,
        None,
        None,
        "admin",
        None,
        None,
        None,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn fault_episode_alerts_once_and_repeated_faulty_checks_do_not_resend() {
    let pool = empty_pool().await;
    seed_admin(&pool, "owner@test.local").await;
    let mail = MailHandle::capture();
    let mut monitor = DoorHealthMonitor::new();

    // Healthy → nothing sent.
    assert_eq!(
        monitor.tick(false, &mail, &pool).await,
        DoorHealthTick::Steady
    );
    assert!(mail.last_captured().is_none(), "no mail while healthy");

    // Rising edge → exactly one fault e-mail, delivered to the single admin.
    assert_eq!(
        monitor.tick(true, &mail, &pool).await,
        DoorHealthTick::FaultAlerted { recipients: 1 }
    );
    let captured = mail.last_captured().expect("fault e-mail must be captured");
    assert_eq!(captured.to, "owner@test.local");
    assert!(
        captured.subject.contains("dvere"),
        "subject: {}",
        captured.subject
    );
    assert!(
        captured.text.contains("vstupy sa nezapisuju"),
        "fault text must name the problem: {}",
        captured.text
    );
    // "Since when": the fault text carries today's gym-local date. Computed
    // via the SAME helper the production code uses (bratislava-tz.md).
    let today = util::now_bratislava().format("%d.%m.%Y").to_string();
    assert!(
        captured.text.contains(&today),
        "fault text must carry the 'since when' date {today}: {}",
        captured.text
    );

    // Still faulty on the next two ticks → NO re-send (the anti-spam dedup).
    assert_eq!(
        monitor.tick(true, &mail, &pool).await,
        DoorHealthTick::Steady,
        "a still-faulty check must not re-send"
    );
    assert_eq!(
        monitor.tick(true, &mail, &pool).await,
        DoorHealthTick::Steady
    );
}

#[tokio::test]
async fn recovery_notifies_once_and_repeated_healthy_checks_do_not_resend() {
    let pool = empty_pool().await;
    seed_admin(&pool, "owner@test.local").await;
    let mail = MailHandle::capture();
    let mut monitor = DoorHealthMonitor::new();

    // Enter the fault episode first.
    assert_eq!(
        monitor.tick(true, &mail, &pool).await,
        DoorHealthTick::FaultAlerted { recipients: 1 }
    );

    // Falling edge → exactly one recovery e-mail.
    assert_eq!(
        monitor.tick(false, &mail, &pool).await,
        DoorHealthTick::Recovered { recipients: 1 }
    );
    let captured = mail
        .last_captured()
        .expect("recovery e-mail must be captured");
    assert_eq!(captured.to, "owner@test.local");
    assert!(
        captured.subject.contains("potvrdzuju"),
        "recovery subject: {}",
        captured.subject
    );
    assert!(
        captured.text.contains("funguje normalne"),
        "recovery text: {}",
        captured.text
    );

    // Steady healthy again → no further mail.
    assert_eq!(
        monitor.tick(false, &mail, &pool).await,
        DoorHealthTick::Steady,
        "a still-healthy check must not re-send a recovery"
    );
}

#[tokio::test]
async fn full_episode_re_arms_for_a_second_fault() {
    let pool = empty_pool().await;
    seed_admin(&pool, "owner@test.local").await;
    let mail = MailHandle::capture();
    let mut monitor = DoorHealthMonitor::new();

    // First episode: fault → recovery.
    assert_eq!(
        monitor.tick(true, &mail, &pool).await,
        DoorHealthTick::FaultAlerted { recipients: 1 }
    );
    assert_eq!(
        monitor.tick(false, &mail, &pool).await,
        DoorHealthTick::Recovered { recipients: 1 }
    );
    // A SECOND fault after recovery must alert again (the episode re-armed).
    assert_eq!(
        monitor.tick(true, &mail, &pool).await,
        DoorHealthTick::FaultAlerted { recipients: 1 },
        "a new fault after recovery is a new episode and must alert again"
    );
}

#[tokio::test]
async fn alert_reaches_every_admin_recipient() {
    let pool = empty_pool().await;
    seed_admin(&pool, "owner-a@test.local").await;
    seed_admin(&pool, "owner-b@test.local").await;
    // A non-admin customer with an e-mail must NOT be alerted.
    users::create_user(
        &pool,
        Some("customer@test.local"),
        None,
        "Customer",
        None,
        None,
        None,
        "customer",
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let mail = MailHandle::capture();
    let mut monitor = DoorHealthMonitor::new();

    assert_eq!(
        monitor.tick(true, &mail, &pool).await,
        DoorHealthTick::FaultAlerted { recipients: 2 },
        "both admins (and only admins) must be alerted"
    );
}

#[tokio::test]
async fn a_failed_send_is_not_deduped_and_retries_next_tick() {
    let pool = empty_pool().await;
    seed_admin(&pool, "owner@test.local").await;
    // `disabled()` makes every send() error — the operational alert must not
    // be silently lost; it retries rather than deduping.
    let mail = MailHandle::disabled();
    let mut monitor = DoorHealthMonitor::new();

    assert_eq!(
        monitor.tick(true, &mail, &pool).await,
        DoorHealthTick::FaultAlertPending,
        "a failed send must report Pending, not a delivered alert"
    );
    assert_eq!(
        monitor.tick(true, &mail, &pool).await,
        DoorHealthTick::FaultAlertPending,
        "a still-undelivered fault must RETRY, not dedup to Steady"
    );
}

#[tokio::test]
async fn no_admin_recipient_yields_pending_not_a_false_delivery() {
    // A DB with an admin whose e-mail is NULL (a card-migrated account) and no
    // deliverable admin → the alert has nowhere to go, so it stays Pending.
    let pool = empty_pool().await;
    users::create_user(
        &pool,
        None, // no e-mail
        None,
        "No-mail admin",
        None,
        None,
        None,
        "admin",
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let mail = MailHandle::capture();
    let mut monitor = DoorHealthMonitor::new();

    assert_eq!(
        monitor.tick(true, &mail, &pool).await,
        DoorHealthTick::FaultAlertPending,
        "no deliverable admin recipient must not read as a delivered alert"
    );
    assert!(mail.last_captured().is_none(), "nothing should be sent");
}

#[tokio::test]
async fn a_failed_recovery_send_is_not_deduped_and_retries_next_tick() {
    // Pins the recovery branch's `n > 0` delivery gate (door_health.rs #355):
    // a failed recovery e-mail must report Pending and keep retrying — a
    // `>` -> `>=` mutation would mark the episode recovered with 0 recipients
    // and silently swallow the recovery notification.
    let pool = empty_pool().await;
    seed_admin(&pool, "owner@test.local").await;
    let good_mail = MailHandle::capture();
    let dead_mail = MailHandle::disabled();
    let mut monitor = DoorHealthMonitor::new();

    // Enter the fault episode with a working relay.
    assert_eq!(
        monitor.tick(true, &good_mail, &pool).await,
        DoorHealthTick::FaultAlerted { recipients: 1 }
    );

    // Recovery while the relay is down -> Pending, retried, never Recovered{0}.
    assert_eq!(
        monitor.tick(false, &dead_mail, &pool).await,
        DoorHealthTick::RecoveryPending,
        "a failed recovery send must report Pending, not a delivered recovery"
    );
    assert_eq!(
        monitor.tick(false, &dead_mail, &pool).await,
        DoorHealthTick::RecoveryPending,
        "a still-undelivered recovery must RETRY, not dedup to Steady"
    );

    // Relay back -> exactly one delivered recovery closes the episode.
    assert_eq!(
        monitor.tick(false, &good_mail, &pool).await,
        DoorHealthTick::Recovered { recipients: 1 }
    );
    assert_eq!(
        monitor.tick(false, &good_mail, &pool).await,
        DoorHealthTick::Steady,
        "after a delivered recovery the episode is closed"
    );
}
