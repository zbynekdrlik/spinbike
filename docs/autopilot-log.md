# Autopilot Log

Terse per-issue log of autonomous work cycles: issue #, commit SHAs, RED→GREEN
test names, decisions, and the shared PR #. Newest entries at the top.

---

## 2026-07-04 — #108: magic-link auth + permanent customer sessions + remove register API

- **Issue:** [#108](https://github.com/zbynekdrlik/spinbike/issues/108) — passwordless invite-only client onboarding + recovery, permanent customer sessions, and removal of the public register API (server side; UI is #109/#111/#112). Validated live before work: no `login_tokens` table existed and `POST /api/auth/register` still worked.
- **Version:** bump `55d21ea` (0.15.0-dev.8 → 0.15.0-dev.9).
- **Migration V17** (`db/migrations.rs`) — `login_tokens` (SHA-256 `token_hash` UNIQUE, purpose CHECK('invite','login'), single-use `used_at`) + index. New `db/login_tokens.rs`: 32-byte base64url raw token (link-only), atomic `UPDATE ... RETURNING user_id` redeem. Tests: `v17_creates_login_tokens_table_with_expected_columns`, `v17_purpose_check_rejects_unknown_value`, `v17_token_hash_is_unique`, `v17_is_idempotent`; module `create_then_redeem_returns_user_id_once`, `reused_token_is_rejected`, `expired_token_is_rejected`, `wrong_purpose_is_rejected_by_scoping`, `ttl_constants_are_exactly_14_days_and_24_hours`.
- **Role-based JWT expiry** (`auth/mod.rs::create_token`) — customer → ~100y, admin/staff → 90d; split test into `jwt_expiry_customer_is_100_years` + `jwt_expiry_admin_and_staff_are_90_days`.
- **Endpoints:** `POST /api/users/{id}/invite` (admin/staff, 503 when mail Disabled, `test_link` in capture mode), `POST /api/auth/request-login-link` (public, uniform-200 no-enumeration, email-keyed rate limiter, **SMTP send `tokio::spawn`'d off the response path** to close a timing side-channel), `POST /api/auth/token-login` (redeem invite/login → JWT, rejects blocked/deleted). Locking tests in `tests/auth_routes.rs`.
- **Register removed** — route+handler+`RegisterRequest` deleted. **Gotcha:** unmatched `/api/*` falls through to the SPA static fallback → 200 index.html, NOT a router 404 — asserted the removed *capability* (no 201, no JWT, no account). Register was also the E2E seed mechanism (`global-setup.ts` + `door-open.spec`) → replaced with a `SPINBIKE_TEST_MODE`-gated `POST /api/test/seed-account` fixture; `auth.spec.ts` register-flow tests reworked (logout now bootstraps via login).
- **Commits:** `55d21ea` (version) → `b8d17c9` (feature) → `0ca6b35` (V17 table-list test) → `3ed4591`/`58de271` (register-removal behavioral assert + fmt) → `d2d7950` (review fixes: per_email prune, spawn-send, doc, coverage) → `643f529` (kill 13 mutation survivors: TTL literals, retain window > decision window, seed-account tests) → `6c8d566` (playbook).
- **Review:** two independent Opus passes (general-purpose deep + `/review` 5-dimension). Deep pass raised a 🔴 for the still-live frontend register CTA — **out of this diff, the map-mandated #112 (verified it covers UI register removal)**; 🟡 unbounded `per_email` map + 🔵 timing side-channel + 🔵 misleading comment all fixed in `d2d7950`.
- **Mutation gate:** diff-scoped `cargo-mutants` found 13 survivors on the first pass (105 mutants, ~70 min); all killed in `643f529` — key lesson: a memory-prune window must be WIDER than the decision window or the boundary mutant is masked (widened `LoginLinkRateLimiter` retain to 120s vs the 60s decision).
- **PR:** [#118](https://github.com/zbynekdrlik/spinbike/pull/118), merged `627c115`.
- **Follow-up filed:** [#119](https://github.com/zbynekdrlik/spinbike/issues/119) — periodic purge of used/expired `login_tokens` rows.
- **Playbook:** new `.claude/skills/auth-onboarding/SKILL.md` + router line; `ci-deploy` skill gained the SPA-fallback-200, seed-account, and mutation-gate learnings.
- **Deployed:** v0.15.0-dev.9, confirmed on both `https://spinbike-dev.newlevel.media` and `https://spinbike.newlevel.media` DOM version labels; `login_tokens` table + schema_version 17 present on both DBs; register creates no account on live prod.

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
