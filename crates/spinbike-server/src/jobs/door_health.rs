//! Door-health fault alerting (#355).
//!
//! #353 left the door physically opening but silently failing to record
//! entries for **two days** and nobody noticed, because the only signal —
//! the `faulty` verdict on `GET /api/door/health` (commit 9dc213e) — is a
//! momentary read nobody watches. This job makes the server itself speak
//! up: it polls that SAME verdict once a minute and e-mails the owner on the
//! healthy→faulty edge, then again on the faulty→healthy edge. Owner
//! decision (2026-08-24): the channel is e-mail via the existing Resend
//! `MailHandle` (the same relay that sends login links); push and an
//! admin-only banner were rejected.
//!
//! ## Why in-memory dedup, not a DB ledger
//!
//! The fault signal (`EwelinkHandle::failed_presses`) is an in-memory
//! counter that RESETS to 0 on every process restart — so the verdict
//! always starts "healthy" after a restart and re-accumulates. The alert
//! dedup state must therefore share that exact lifetime: it lives in the
//! in-memory `DoorHealthMonitor`. A DB row that outlived the process would
//! misread a restart's counter reset as a "recovery" and send a FALSE
//! recovery e-mail. The "record in time" the ticket asks for is served by
//! the alert e-mails themselves plus the timestamped `tracing` lines below
//! (an in-app queryable history would be a separate needs-user-decision
//! follow-up — the owner chose e-mail as the single channel).
//!
//! ## Dedup semantics
//!
//! Exactly one e-mail per fault episode in each direction. `alerted` flips
//! false→true only after a fault e-mail is SUCCESSFULLY delivered to at
//! least one admin, and true→false only after a recovery e-mail is — the
//! same "stamp only on Ok" rule the push-notify ledger uses
//! (`.claude/rules/push-notifications.md`). A transient SMTP failure leaves
//! the flag unchanged so the next tick retries, rather than silently losing
//! an operational alert (the whole failure #355 exists to prevent).

use crate::db::users;
use crate::mail::MailHandle;
use chrono::NaiveDateTime;
use sqlx::SqlitePool;

/// Human-readable timestamp format for the e-mail bodies (gym-local time).
const DT_FMT: &str = "%d.%m.%Y %H:%M";

/// The outcome of one `DoorHealthMonitor::tick` — every variant is a
/// distinct, testable transition so the anti-spam behaviour can be pinned
/// exactly (one alert per episode, retries on delivery failure).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoorHealthTick {
    /// No verdict edge this tick (steady healthy, or steady faulty after the
    /// episode was already alerted) — nothing sent.
    Steady,
    /// Rising edge (healthy→faulty): the owner was successfully alerted this
    /// tick. Carries how many admin recipients received the mail.
    FaultAlerted { recipients: usize },
    /// Rising edge detected, but the alert e-mail reached no admin (SMTP
    /// failure, or no admin recipient) — retried on the next tick.
    FaultAlertPending,
    /// Falling edge (faulty→healthy): the owner was successfully notified.
    Recovered { recipients: usize },
    /// Falling edge detected, but the recovery e-mail reached no admin —
    /// retried on the next tick.
    RecoveryPending,
}

/// In-memory anti-spam state for the door-fault alert. Lifetime matches the
/// in-memory fault signal it tracks (both reset on restart) — see the
/// module docs for why this is deliberately NOT persisted to the DB.
pub struct DoorHealthMonitor {
    /// True once a fault e-mail has been successfully delivered for the
    /// CURRENT episode; reset once a recovery e-mail has been delivered.
    alerted: bool,
    /// Gym-local wall-clock time the current fault was first DETECTED, for
    /// the "porucha trvala od …" line in the recovery mail. `None` while
    /// healthy / not-yet-alerted.
    faulty_since: Option<NaiveDateTime>,
}

impl Default for DoorHealthMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl DoorHealthMonitor {
    pub fn new() -> Self {
        Self {
            alerted: false,
            faulty_since: None,
        }
    }

    /// Feed the current door-fault verdict (`EwelinkHandle::is_faulty()`) in.
    /// Takes the verdict as a plain `bool` rather than the whole ewelink
    /// handle (dependency inversion) so the state machine is testable with
    /// no WS/hardware handle at all. Sends at most one e-mail per call, only
    /// on a verdict EDGE.
    pub async fn tick(
        &mut self,
        now_faulty: bool,
        mail: &MailHandle,
        pool: &SqlitePool,
    ) -> DoorHealthTick {
        if now_faulty && !self.alerted {
            let detected = crate::util::now_bratislava();
            let n = send_alert(pool, mail, &AlertKind::Fault { detected }).await;
            if n > 0 {
                self.alerted = true;
                self.faulty_since = Some(detected);
                tracing::error!(
                    recipients = n,
                    "door_health: FAULT detected — owner alerted by e-mail"
                );
                DoorHealthTick::FaultAlerted { recipients: n }
            } else {
                tracing::error!(
                    "door_health: FAULT detected but the alert e-mail reached no admin; \
                     retrying next tick"
                );
                DoorHealthTick::FaultAlertPending
            }
        } else if !now_faulty && self.alerted {
            let recovered = crate::util::now_bratislava();
            let n = send_alert(
                pool,
                mail,
                &AlertKind::Recovery {
                    since: self.faulty_since,
                    recovered,
                },
            )
            .await;
            if n > 0 {
                self.alerted = false;
                self.faulty_since = None;
                tracing::info!(
                    recipients = n,
                    "door_health: recovered — owner notified by e-mail"
                );
                DoorHealthTick::Recovered { recipients: n }
            } else {
                tracing::error!(
                    "door_health: recovered but the recovery e-mail reached no admin; \
                     retrying next tick"
                );
                DoorHealthTick::RecoveryPending
            }
        } else {
            DoorHealthTick::Steady
        }
    }
}

/// Which alert to compose + send.
enum AlertKind {
    Fault {
        detected: NaiveDateTime,
    },
    Recovery {
        since: Option<NaiveDateTime>,
        recovered: NaiveDateTime,
    },
}

/// Load the admin recipients, compose the mail, send to each. Returns the
/// number of admins that received it (0 = a DB error, no admin with an
/// e-mail, or every send failed — all logged, all retried next tick).
async fn send_alert(pool: &SqlitePool, mail: &MailHandle, kind: &AlertKind) -> usize {
    let recipients = match users::admin_alert_recipients(pool).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, "door_health: could not load admin recipients");
            return 0;
        }
    };
    if recipients.is_empty() {
        tracing::error!("door_health: no admin recipient with an e-mail address configured");
        return 0;
    }
    let (subject, text, html) = compose(kind);
    let mut sent = 0usize;
    for to in &recipients {
        match mail.send(to, &subject, &text, &html).await {
            Ok(()) => sent += 1,
            // The mail module already logs the address + error on a real
            // send; keep this line address-free (kind + count is enough here).
            Err(e) => tracing::error!(error = %e, "door_health: alert e-mail to an admin failed"),
        }
    }
    sent
}

/// Pure — compose `(subject, text, html)` for an alert. Slovak WITHOUT
/// diacritics (project rule for user-facing strings), short, and states what
/// is wrong plus since when.
fn compose(kind: &AlertKind) -> (String, String, String) {
    match kind {
        AlertKind::Fault { detected } => {
            let ts = detected.format(DT_FMT).to_string();
            let subject = "SpinBike: dvere nepotvrdzuju vstupy".to_string();
            let text = format!(
                "Dvere sa fyzicky otvaraju, ale potvrdenia od zariadenia uz neprichadzaju - \
                 vstupy sa nezapisuju.\n\n\
                 Porucha zistena: {ts} (cas Bratislava).\n\n\
                 Kym to trva, klientovi pri stlaceni vyskoci chyba a vstup treba dopisat rucne. \
                 Skontroluj prosim dvere a pripojenie eWeLink."
            );
            let html = format!(
                "<p>Dvere sa fyzicky otvaraju, ale potvrdenia od zariadenia uz neprichadzaju - \
                 <strong>vstupy sa nezapisuju</strong>.</p>\
                 <p>Porucha zistena: <strong>{ts}</strong> (cas Bratislava).</p>\
                 <p>Kym to trva, klientovi pri stlaceni vyskoci chyba a vstup treba dopisat rucne. \
                 Skontroluj prosim dvere a pripojenie eWeLink.</p>"
            );
            (subject, text, html)
        }
        AlertKind::Recovery { since, recovered } => {
            let rec = recovered.format(DT_FMT).to_string();
            let subject = "SpinBike: dvere zase potvrdzuju vstupy".to_string();
            let when = match since {
                Some(s) => format!("Porucha trvala od {} do {}.", s.format(DT_FMT), rec),
                None => format!("Zotavene: {rec} (cas Bratislava)."),
            };
            let text = format!(
                "Dvere zase potvrdzuju vstupy - zapisovanie vstupov funguje normalne.\n\n{when}"
            );
            let html = format!(
                "<p>Dvere zase potvrdzuju vstupy - zapisovanie vstupov funguje normalne.</p>\
                 <p>{when}</p>"
            );
            (subject, text, html)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, mo, d)
            .unwrap()
            .and_hms_opt(h, mi, 0)
            .unwrap()
    }

    /// Every user-facing string must be plain Slovak WITHOUT diacritics
    /// (project rule) — assert pure ASCII across subject/text/html of both
    /// mails.
    #[test]
    fn compose_bodies_are_ascii_only_no_diacritics() {
        for kind in [
            AlertKind::Fault {
                detected: dt(2026, 8, 24, 14, 30),
            },
            AlertKind::Recovery {
                since: Some(dt(2026, 8, 24, 14, 30)),
                recovered: dt(2026, 8, 24, 16, 5),
            },
        ] {
            let (subject, text, html) = compose(&kind);
            assert!(subject.is_ascii(), "subject has non-ASCII: {subject:?}");
            assert!(text.is_ascii(), "text has non-ASCII: {text:?}");
            assert!(html.is_ascii(), "html has non-ASCII: {html:?}");
        }
    }

    /// The fault mail must say what is wrong AND since when.
    #[test]
    fn fault_mail_states_the_problem_and_since_when() {
        let (subject, text, _html) = compose(&AlertKind::Fault {
            detected: dt(2026, 8, 24, 14, 30),
        });
        assert!(subject.contains("dvere"), "subject: {subject}");
        assert!(
            text.contains("vstupy sa nezapisuju"),
            "fault text must name the actual problem: {text}"
        );
        assert!(
            text.contains("24.08.2026 14:30"),
            "fault text must carry the 'since when' timestamp: {text}"
        );
    }

    /// The recovery mail must say it recovered AND how long the fault lasted.
    #[test]
    fn recovery_mail_states_recovery_and_the_fault_interval() {
        let (subject, text, _html) = compose(&AlertKind::Recovery {
            since: Some(dt(2026, 8, 24, 14, 30)),
            recovered: dt(2026, 8, 24, 16, 5),
        });
        assert!(subject.contains("potvrdzuju"), "subject: {subject}");
        assert!(
            text.contains("24.08.2026 14:30") && text.contains("24.08.2026 16:05"),
            "recovery text must carry both fault-start and recovery timestamps: {text}"
        );
    }

    /// A recovery with no recorded start (defensive; should not happen since
    /// recovery only fires after a fault set `faulty_since`) still composes
    /// sensibly with the recovery timestamp.
    #[test]
    fn recovery_mail_without_since_still_names_the_recovery_time() {
        let (_subject, text, _html) = compose(&AlertKind::Recovery {
            since: None,
            recovered: dt(2026, 8, 24, 16, 5),
        });
        assert!(text.contains("24.08.2026 16:05"), "text: {text}");
    }
}
