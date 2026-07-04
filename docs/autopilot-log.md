# Autopilot Log

Terse per-issue log of autonomous work cycles: issue #, commit SHAs, RED→GREEN
test names, decisions, and the shared PR #. Newest entries at the top.

---

## 2026-07-04 — #106: door blocked-users gate

- **Issue:** [#106](https://github.com/zbynekdrlik/spinbike/issues/106) — `POST /api/door/open` never checked `users.blocked`; a blocked customer with `allow_self_entry=1` (or a blocked admin/staff, who bypass the `allow_self_entry` gate entirely) could still actuate the relay and get billed.
- **Fix:** added `blocked` to the door precondition SELECT, rejected with `403 {"status":"rejected","reason":"blocked"}` before the allow_self_entry role bypass, the rate limiter, the tx insert, and the relay press — for every role.
- **Commits:** version bump `791bbf0` (0.15.0-dev.6 → 0.15.0-dev.7) → RED `683f540` (`crates/spinbike-server/tests/door_route.rs::blocked_customer_with_allow_self_entry_is_rejected` + `blocked_admin_is_rejected_despite_role_bypass` + `blocked_staff_is_rejected_despite_role_bypass`) → GREEN `4046370` (`crates/spinbike-server/src/routes/door.rs`) → polish `a9abf23` (review-feedback: health-endpoint assertion symmetry + reason-tag comment).
- **Decision (yours to make, per issue text, already settled — not re-asked):** blocked-means-blocked for ALL roles including admin/staff, even though admin/staff still bypass `allow_self_entry`. Reason tag `"blocked"` reuses door.rs's own `{"status":"rejected","reason":"<tag>"}` envelope (matches its existing `not_allowed`/`rate_limited` shape and users.rs's 403 precedent) rather than payments.rs's `{"error": "User is blocked"}` + 409 shape — documented inline in door.rs so a future reader doesn't "fix" the intentional inconsistency.
- **Review:** two independent passes (general-purpose + `superpowers:requesting-code-review`), both 0 Critical / 0 Important; 2 Minor items addressed in the polish commit before merge.
- **Live verification:** synthetic test users (created + JWT-signed + cleaned up, zero real customer data touched) on BOTH dev and prod confirmed blocked-customer, blocked-admin → 403 `blocked`, zero tx rows, `last_ack_ms_ago` stayed null (relay never pressed); unblocked control customer on dev correctly passed the gate and reached the relay call (503 `hardware_unavailable` — expected, dev has no eWeLink creds by design).
- **PR:** [#114](https://github.com/zbynekdrlik/spinbike/pull/114), merged `68d37b3`.
- **Follow-up filed:** [#113](https://github.com/zbynekdrlik/spinbike/issues/113) — pre-existing Trunk-generated console warning (preload `integrity` attribute), unrelated to this fix, found during post-deploy verification.
- **Deployed:** v0.15.0-dev.7, confirmed on both `https://spinbike-dev.newlevel.media` and `https://spinbike.newlevel.media` DOM version labels.
