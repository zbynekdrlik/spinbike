---
paths:
  - "crates/spinbike-server/src/jobs/notifications.rs"
  - "crates/spinbike-server/src/push.rs"
  - "crates/spinbike-server/src/db/push.rs"
  - "crates/spinbike-server/src/routes/push.rs"
  - "spinbike-ui/sw.js"
---

# Web-Push notifications (#264)

## Shape

- `jobs::notifications::tick` runs **once at startup** and then on a sleep loop
  aligned to `DAILY_RUN_HOUR = 9` (Europe/Bratislava). There is **no admin/debug
  HTTP endpoint and no standalone bin** that fires it — restarting the unit is
  the only way to force a tick.
- Two reasons, gated independently:
  - `low_credit` — `credit <= LOW_CREDIT_THRESHOLD_EUR (3.30)` **AND**
    `last_topup >= MIN_LAST_TOPUP_EUR (20.0)` **AND NOT** an active monthly
    pass (`db::users::get_user_pass_valid_until >= today`, #306). The top-up
    gate is the owner's explicit rule: a one-off single-entry customer must
    never be nagged. It reads the **single most recent** credit-increasing
    transaction (`action='topup' AND amount>0 AND deleted_at IS NULL`,
    `ORDER BY created_at DESC, id DESC LIMIT 1`) — never a sum. The
    active-pass gate exists because `credit` is 0 BY DESIGN while a pass is
    active (visits during the pass period are booked `action='visit'`
    amount 0) — without it, a pass holder whose last top-up cleared the 20
    EUR gate got a nonsensical "Dochadza ti kredit" push (proven live on
    prod: 26 affected customers, including the owner's own account).
  - `pass_expiring` — `PASS_EXPIRING_DAYS = 3`. **Not** subject to the 20 EUR
    gate.
  - **Structural invariant (#306): `low_credit` and `pass_expiring` can
    never co-fire for the SAME user.** `pass_expiring`'s own window
    (`valid_until` in `[today, today+3]`) is a strict subset of
    `low_credit`'s active-pass suppression condition (`valid_until >=
    today`) — so any user for whom `pass_expiring` fires necessarily has
    `low_credit` suppressed. If a future change to either condition makes
    this invariant NOT hold anymore, re-check
    `low_credit_and_pass_expiring_can_never_co_fire_for_the_same_user` in
    `notifications.rs` (it locks this exact invariant with `sent == 1`,
    not `sent == 2`).
- Anti-spam state is per user **per reason** in `push_notify_log(user_id, reason,
  last_notified_at, sent_count)`: `NOTIFY_COOLDOWN_DAYS = 7`,
  `MAX_NOTIFICATIONS_PER_EPISODE = 2`, re-armed (both columns reset) when the
  condition clears.
- **E-mail FALLBACK (#311, owner decision variant (a), 2026-08-10): e-mail is
  never a duplicate channel, only a fallback.** In `evaluate_reason`, the
  choice of channel is made on whether the customer has ANY stored push
  subscription **at all** (`db::push::list_subscriptions_for_user(...).is_empty()`)
  — NOT on whether push delivery succeeded on THIS tick. A subscription that's
  merely failing transiently still gets push-only treatment for as long as it
  exists; the pre-existing `MAX_CONSECUTIVE_FAILURES` pruning is what
  eventually moves a genuinely dead subscription's owner onto the e-mail path,
  on a later tick — there is no separate "push failed this tick, try e-mail
  too" branch, by design (would blur the "never both" guarantee). No stored
  subscription + `users.email` present -> exactly one e-mail via
  `MailHandle::send`, subject/text identical to the push title/body, html a
  plain `<p>` wrap. No subscription + `users.email IS NULL` (typical for
  card-migrated legacy accounts) -> skip silently, same as "no subscription"
  always behaved. The ledger is stamped only on `Ok(())` from `send()` — a
  failed SMTP send never stamps it, mirroring the push `SendOutcome::Sent`-only
  stamping rule above.

## The two gotchas that decide how you can test it

1. **`push::send()` performs no host validation.** The
   `fcm.googleapis.com` / `updates.push.services.mozilla.com` /
   `web.push.apple.com` / `*.notify.windows.com` allowlist lives **only in the
   `/api/push/subscribe` route** (SSRF guard at the boundary). A subscription
   seeded straight into the DB therefore reaches any endpoint the row names.
2. **The anti-spam ledger is stamped ONLY on `SendOutcome::Sent` (a real 2xx).**
   A failed or retryable send is retried on *every* tick with no cooldown until
   `MAX_CONSECUTIVE_FAILURES = 10` prunes the subscription.

Consequence: **a failed POST proves selection but can never prove the cooldown.**
To verify anti-spam end-to-end you need a real 2xx, which a fabricated
subscription will never get from a real push service. Point the seeded
subscription at a **local mock HTTP server returning 201**, and use the
`web-push` crate's own public test-fixture `p256dh`/`auth` keypair so RFC-8291
encryption actually succeeds and a genuine VAPID-signed request is emitted.

## VAPID key

One secret only: `VAPID_PRIVATE_KEY` — a 32-byte raw P-256 private key,
base64url **unpadded** (exactly 43 chars). The public key is derived at startup
via `PartialVapidSignatureBuilder::get_public_key()` and served to the frontend
by the auth-gated `GET /api/push/config`. Prod and dev hold **distinct** keys in
their own `EnvironmentFile`; never print or copy the value. A wrong-length key
used to panic inside `generic-array` — `push.rs` now length-validates first.

## Verifying on prod

Restart the unit and read the journal: `push: notified user_id=<id>
reason="low_credit"` then `push: daily tick complete sent=<n>`. A tick that
selected nobody logs `sent=0` with no `notified` line. A successful e-mail
fallback send logs `mail: sent to=<addr> subject=<title>` (from
`mail::send_via_transport`) right before the SAME `push: notified` line —
`evaluate_reason` doesn't distinguish the log line by channel, only
`mail`/`push`'s own modules do. A failed e-mail fallback logs `push: email
fallback send failed, ledger not stamped (retried next tick)` with the
underlying `MailError`. Delete every synthetic row afterwards (`users`,
`transactions`, `push_subscriptions`, `push_notify_log`) — prod holds real
customer data.
