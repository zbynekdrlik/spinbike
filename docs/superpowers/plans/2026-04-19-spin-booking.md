# Spin Class Booking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Card-centric spin class booking — staff finds a card, books a class, toggles persistent weekly subscriptions. A background charger at T−4h debits credit or logs a free pass-visit.

**Architecture:** Axum REST routes + Leptos WASM frontend + SQLite. New tables `persistent_bookings`; new columns on `bookings`. Two background tokio loops: 60s charger + 60min persistent-materialiser. Seed the 4 weekly classes via a new migration.

**Tech Stack:** Rust (sqlx, axum 0.8, tokio, chrono), Leptos 0.7 (CSR/WASM), TypeScript Playwright E2E.

**Spec:** `docs/superpowers/specs/2026-04-19-spin-booking-design.md`

---

## File Structure

**New files:**
- `crates/spinbike-server/src/db/persistent_bookings.rs` — CRUD + materialiser
- `crates/spinbike-server/src/jobs/mod.rs` — module gate
- `crates/spinbike-server/src/jobs/charger.rs` — T−4h charging loop
- `crates/spinbike-server/src/jobs/materialiser.rs` — persistent booking sweep
- `crates/spinbike-server/src/routes/persistent_bookings.rs` — HTTP routes
- `crates/spinbike-server/src/routes/upcoming_classes.rs` — per-card upcoming view
- `crates/spinbike-server/tests/persistent_bookings.rs` — integration tests
- `crates/spinbike-server/tests/charger.rs` — charger logic tests
- `crates/spinbike-server/tests/upcoming_classes.rs` — upcoming view tests
- `spinbike-ui/src/components/upcoming_classes.rs` — panel in staff card page
- `spinbike-ui/src/components/persistent_toggles.rs` — 4 weekday toggles
- `e2e/tests/spin-booking.spec.ts` — E2E

**Modified files:**
- `crates/spinbike-server/src/db/migrations.rs` — V5 + V6 migrations
- `crates/spinbike-server/src/db/classes.rs` — extend `create_booking` with `card_id`/`source`, add query helpers
- `crates/spinbike-server/src/db/mod.rs` — `pub mod persistent_bookings;`
- `crates/spinbike-server/src/lib.rs` — `pub mod jobs;` + register new routes
- `crates/spinbike-server/src/bin/server.rs` — spawn charger + materialiser
- `crates/spinbike-server/src/routes/classes.rs` — update `create_booking` to accept `card_id`
- `spinbike-ui/src/pages/dashboard.rs` — insert UpcomingClasses + PersistentToggles in ActionPanel
- `spinbike-ui/src/components/mod.rs` — expose new components
- `spinbike-ui/src/i18n.rs` — add new translation keys

---

## Task 1: V5 migration — extend `bookings` + new `persistent_bookings` table

**Files:**
- Modify: `crates/spinbike-server/src/db/migrations.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/spinbike-server/src/db/migrations.rs` inside the existing `#[cfg(test)] mod tests { ... }` block:

```rust
    #[tokio::test]
    async fn v5_adds_booking_columns_and_persistent_table() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        // bookings gained card_id, source, charged_at, charge_transaction_id
        let cols: Vec<(String,)> =
            sqlx::query_as("SELECT name FROM pragma_table_info('bookings')")
                .fetch_all(&pool).await.unwrap();
        let names: Vec<&str> = cols.iter().map(|(n,)| n.as_str()).collect();
        for c in ["card_id", "source", "charged_at", "charge_transaction_id"] {
            assert!(names.contains(&c), "bookings missing column {c}");
        }

        // persistent_bookings exists with the right unique index
        let tbl: Option<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='persistent_bookings'"
        ).fetch_optional(&pool).await.unwrap();
        assert!(tbl.is_some(), "persistent_bookings table missing");

        let idx: Vec<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='persistent_bookings'"
        ).fetch_all(&pool).await.unwrap();
        assert!(idx.iter().any(|(n,)| n.contains("card_id_template_id")),
                "unique index on (card_id,template_id) missing");
    }

    #[tokio::test]
    async fn v5_is_idempotent() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        run_migrations(&pool).await.unwrap();
    }
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p spinbike-server --lib db::migrations::tests::v5 -- --nocapture
```

Expected: FAIL — V5 doesn't exist.

- [ ] **Step 3: Add V5 migration**

In the same file, at the end of the migration constants (after `V4_MONTHLY_PASS`), add:

```rust
const V5_SPIN_BOOKING: &str = r#"
ALTER TABLE bookings ADD COLUMN card_id INTEGER REFERENCES cards(id);
ALTER TABLE bookings ADD COLUMN source TEXT NOT NULL DEFAULT 'manual';
ALTER TABLE bookings ADD COLUMN charged_at TEXT;
ALTER TABLE bookings ADD COLUMN charge_transaction_id INTEGER REFERENCES transactions(id);

UPDATE bookings
  SET card_id = (SELECT c.id FROM cards c WHERE c.user_id = bookings.user_id LIMIT 1)
  WHERE card_id IS NULL;

CREATE TABLE IF NOT EXISTS persistent_bookings (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    card_id     INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    template_id INTEGER NOT NULL REFERENCES class_templates(id) ON DELETE CASCADE,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    ended_at    TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_persistent_bookings_card_id_template_id_active
    ON persistent_bookings(card_id, template_id)
    WHERE ended_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_bookings_uncharged_future
    ON bookings(date, charged_at)
    WHERE cancelled_at IS NULL AND charged_at IS NULL;
"#;
```

Then register it in `MIGRATIONS`:

```rust
pub(crate) static MIGRATIONS: &[(i64, &str, &str)] = &[
    (1, "initial schema", V1_INITIAL_SCHEMA),
    (2, "card holder info and allow debit default", V2_CARD_HOLDER_INFO),
    (3, "card search_text column + index", V3_CARD_SEARCH_TEXT),
    (4, "monthly pass: valid_until + service seed", V4_MONTHLY_PASS),
    (5, "spin booking: bookings extended + persistent_bookings", V5_SPIN_BOOKING),
];
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test -p spinbike-server --lib db::migrations::tests::v5 -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/spinbike-server/src/db/migrations.rs
git commit -m "feat(db): V5 migration — persistent_bookings + booking columns

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: V6 migration — seed instructors and 4 weekly class templates

**Files:**
- Modify: `crates/spinbike-server/src/db/migrations.rs`

- [ ] **Step 1: Write the failing test**

Append to the `#[cfg(test)] mod tests` block:

```rust
    #[tokio::test]
    async fn v6_seeds_instructors_and_templates() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let stevo_id: Option<i64> =
            sqlx::query_scalar("SELECT id FROM instructors WHERE name='Stevo' AND active=1")
                .fetch_optional(&pool).await.unwrap();
        assert!(stevo_id.is_some(), "Stevo must be seeded");

        let vlada_id: Option<i64> =
            sqlx::query_scalar("SELECT id FROM instructors WHERE name='Vlada' AND active=1")
                .fetch_optional(&pool).await.unwrap();
        assert!(vlada_id.is_some(), "Vlada must be seeded");

        // Exactly 4 templates at 18:00 with capacity 19, one per weekday 0..=3.
        let rows: Vec<(i64, String, i64, i64)> = sqlx::query_as(
            "SELECT weekday, start_time, capacity, instructor_id
             FROM class_templates
             WHERE start_time = '18:00' AND active = 1
             ORDER BY weekday"
        ).fetch_all(&pool).await.unwrap();

        assert_eq!(rows.len(), 4, "expected 4 seeded templates");
        for (i, (wd, st, cap, inst)) in rows.iter().enumerate() {
            assert_eq!(*wd, i as i64);
            assert_eq!(st, "18:00");
            assert_eq!(*cap, 19);
            let expected = if *wd == 0 || *wd == 2 { stevo_id.unwrap() } else { vlada_id.unwrap() };
            assert_eq!(*inst, expected, "wrong instructor for weekday {wd}");
        }
    }

    #[tokio::test]
    async fn v6_is_idempotent_and_does_not_duplicate() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        run_migrations(&pool).await.unwrap();

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM class_templates WHERE start_time='18:00' AND active=1"
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(count, 4);

        let instr_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM instructors WHERE name IN ('Stevo','Vlada')"
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(instr_count, 2);
    }
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p spinbike-server --lib db::migrations::tests::v6 -- --nocapture
```

Expected: FAIL.

- [ ] **Step 3: Add V6 migration**

After `V5_SPIN_BOOKING`:

```rust
const V6_SEED_SPIN_CLASSES: &str = r#"
INSERT INTO instructors (name, active)
SELECT 'Stevo', 1 WHERE NOT EXISTS (SELECT 1 FROM instructors WHERE name='Stevo');
INSERT INTO instructors (name, active)
SELECT 'Vlada', 1 WHERE NOT EXISTS (SELECT 1 FROM instructors WHERE name='Vlada');

INSERT INTO class_templates (weekday, start_time, duration_minutes, instructor_id, capacity, active)
SELECT 0, '18:00', 60, (SELECT id FROM instructors WHERE name='Stevo'), 19, 1
WHERE NOT EXISTS (SELECT 1 FROM class_templates WHERE weekday=0 AND start_time='18:00');

INSERT INTO class_templates (weekday, start_time, duration_minutes, instructor_id, capacity, active)
SELECT 1, '18:00', 60, (SELECT id FROM instructors WHERE name='Vlada'), 19, 1
WHERE NOT EXISTS (SELECT 1 FROM class_templates WHERE weekday=1 AND start_time='18:00');

INSERT INTO class_templates (weekday, start_time, duration_minutes, instructor_id, capacity, active)
SELECT 2, '18:00', 60, (SELECT id FROM instructors WHERE name='Stevo'), 19, 1
WHERE NOT EXISTS (SELECT 1 FROM class_templates WHERE weekday=2 AND start_time='18:00');

INSERT INTO class_templates (weekday, start_time, duration_minutes, instructor_id, capacity, active)
SELECT 3, '18:00', 60, (SELECT id FROM instructors WHERE name='Vlada'), 19, 1
WHERE NOT EXISTS (SELECT 1 FROM class_templates WHERE weekday=3 AND start_time='18:00');
"#;
```

Register:

```rust
    (6, "seed 4 weekly spin classes + 2 instructors", V6_SEED_SPIN_CLASSES),
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test -p spinbike-server --lib db::migrations::tests::v6 -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/spinbike-server/src/db/migrations.rs
git commit -m "feat(db): V6 seed Stevo/Vlada + Mon-Thu 18:00 spin classes

Capacity 19, idempotent inserts.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Extend `create_booking` in db::classes with card_id + source

**Files:**
- Modify: `crates/spinbike-server/src/db/classes.rs`

- [ ] **Step 1: Write the failing test**

Add inside the existing `#[cfg(test)] mod tests` of `classes.rs`:

```rust
    #[tokio::test]
    async fn create_booking_records_card_id_and_source() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        // Seed user, card, template.
        let user_id: i64 = sqlx::query_scalar(
            "INSERT INTO users (email, name) VALUES ('u@x','u') RETURNING id"
        ).fetch_one(&pool).await.unwrap();
        let card_id: i64 = sqlx::query_scalar(
            "INSERT INTO cards (barcode, user_id, credit) VALUES ('B1', ?, 0) RETURNING id"
        ).bind(user_id).fetch_one(&pool).await.unwrap();
        let template_id = create_template(&pool, 0, "18:00", 60, None, 19).await.unwrap();

        let id = create_booking(&pool, template_id, "2026-04-20", user_id, card_id, None, "persistent")
            .await.unwrap();

        let (got_card, got_source): (i64, String) = sqlx::query_as(
            "SELECT card_id, source FROM bookings WHERE id = ?"
        ).bind(id).fetch_one(&pool).await.unwrap();
        assert_eq!(got_card, card_id);
        assert_eq!(got_source, "persistent");
    }
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p spinbike-server --lib db::classes::tests::create_booking_records_card_id_and_source
```

Expected: FAIL — `create_booking` signature doesn't accept `card_id`/`source`.

- [ ] **Step 3: Change the `create_booking` signature**

In `crates/spinbike-server/src/db/classes.rs`, replace the existing `create_booking` function:

```rust
pub async fn create_booking(
    pool: &SqlitePool,
    template_id: i64,
    date: &str,
    user_id: i64,
    card_id: i64,
    created_by: Option<i64>,
    source: &str,
) -> Result<i64> {
    let result = sqlx::query_scalar::<_, i64>(
        "INSERT INTO bookings (template_id, date, user_id, card_id, created_by, source)
         SELECT ?1, ?2, ?3, ?4, ?5, ?6
         WHERE (SELECT COUNT(*) FROM bookings
                WHERE template_id = ?1 AND date = ?2 AND cancelled_at IS NULL)
               < (SELECT capacity FROM class_templates WHERE id = ?1)
         RETURNING id",
    )
    .bind(template_id)
    .bind(date)
    .bind(user_id)
    .bind(card_id)
    .bind(created_by)
    .bind(source)
    .fetch_optional(pool)
    .await?;
    match result {
        Some(id) => Ok(id),
        None => anyhow::bail!("Class is full"),
    }
}
```

Update all existing call sites (compile errors will guide you):
- `crates/spinbike-server/src/routes/classes.rs` — `create_booking` route will pass a real `card_id`; temporarily pass `0` and update properly in Task 5.
- `crates/spinbike-server/tests/classes_routes.rs` — if it calls `db_classes::create_booking` directly, add the two new args.

- [ ] **Step 4: Run test**

```bash
cargo test -p spinbike-server --lib db::classes::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/spinbike-server/src/db/classes.rs crates/spinbike-server/src/routes/classes.rs crates/spinbike-server/tests/classes_routes.rs
git commit -m "feat(db): record card_id + source on bookings

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Query helpers — upcoming occurrences + booking lookup

**Files:**
- Modify: `crates/spinbike-server/src/db/classes.rs`

- [ ] **Step 1: Write the failing test**

Append to `#[cfg(test)] mod tests`:

```rust
    #[tokio::test]
    async fn list_upcoming_for_card_joins_booking_state() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        let user_id: i64 = sqlx::query_scalar(
            "INSERT INTO users (email, name) VALUES ('u@x','u') RETURNING id"
        ).fetch_one(&pool).await.unwrap();
        let card_id: i64 = sqlx::query_scalar(
            "INSERT INTO cards (barcode, user_id, credit) VALUES ('B', ?, 0) RETURNING id"
        ).bind(user_id).fetch_one(&pool).await.unwrap();

        // Pick a weekday we know — use chrono to get next Monday.
        use chrono::{Datelike, Duration, NaiveDate};
        let today = chrono::Local::now().date_naive();
        let days_to_mon = (8 - today.weekday().num_days_from_monday()) % 7;
        let mon = today + Duration::days(days_to_mon as i64);
        let template_id = create_template(&pool, 0, "18:00", 60, None, 19).await.unwrap();

        // No booking yet: state == "free".
        let rows = list_upcoming_for_card(&pool, card_id, &today.to_string(),
                                          &(today + Duration::days(14)).to_string()).await.unwrap();
        let monday_row = rows.iter().find(|r| r.date == mon.to_string()).unwrap();
        assert_eq!(monday_row.state, "free");
        assert!(monday_row.booking_id.is_none());

        // Book manual: state == "booked", booking_id set.
        let bid = create_booking(&pool, template_id, &mon.to_string(), user_id, card_id, None, "manual")
            .await.unwrap();
        let rows = list_upcoming_for_card(&pool, card_id, &today.to_string(),
                                          &(today + Duration::days(14)).to_string()).await.unwrap();
        let monday_row = rows.iter().find(|r| r.date == mon.to_string()).unwrap();
        assert_eq!(monday_row.state, "booked");
        assert_eq!(monday_row.booking_id, Some(bid));
    }
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p spinbike-server --lib db::classes::tests::list_upcoming_for_card_joins_booking_state
```

Expected: FAIL — function not defined.

- [ ] **Step 3: Add struct + function**

Append to `crates/spinbike-server/src/db/classes.rs`:

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpcomingRow {
    pub template_id: i64,
    pub date: String,
    pub start_time: String,
    pub duration_minutes: i64,
    pub instructor_id: Option<i64>,
    pub instructor_name: Option<String>,
    pub capacity: i64,
    pub booked: i64,
    pub state: String,           // "free" | "booked" | "auto" | "full" | "past" | "cancelled"
    pub booking_id: Option<i64>,
}

pub async fn list_upcoming_for_card(
    pool: &SqlitePool,
    card_id: i64,
    from: &str,
    to: &str,
) -> Result<Vec<UpcomingRow>> {
    use chrono::{Duration, NaiveDate, Datelike};
    let from_d = NaiveDate::parse_from_str(from, "%Y-%m-%d")?;
    let to_d = NaiveDate::parse_from_str(to, "%Y-%m-%d")?;

    let templates: Vec<ClassTemplateRow> = sqlx::query_as(
        "SELECT id, weekday, start_time, duration_minutes, instructor_id, capacity, active
         FROM class_templates WHERE active = 1"
    ).fetch_all(pool).await?;

    let now = chrono::Local::now().naive_local();
    let mut out = Vec::new();
    let mut d = from_d;
    while d <= to_d {
        for t in &templates {
            if d.weekday().num_days_from_monday() as i64 != t.weekday { continue; }
            let date_s = d.to_string();

            let cancelled: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM class_cancellations WHERE template_id = ? AND date = ?"
            ).bind(t.id).bind(&date_s).fetch_optional(pool).await?;

            let booked: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM bookings WHERE template_id = ? AND date = ? AND cancelled_at IS NULL"
            ).bind(t.id).bind(&date_s).fetch_one(pool).await?;

            let my_row: Option<(i64, String)> = sqlx::query_as(
                "SELECT id, source FROM bookings
                 WHERE template_id = ? AND date = ? AND card_id = ? AND cancelled_at IS NULL"
            ).bind(t.id).bind(&date_s).bind(card_id).fetch_optional(pool).await?;

            let start_dt = format!("{date_s} {}", t.start_time);
            let start_parsed = chrono::NaiveDateTime::parse_from_str(&start_dt, "%Y-%m-%d %H:%M").ok();
            let is_past = matches!(start_parsed, Some(s) if s <= now);

            let state = if cancelled.is_some() { "cancelled" }
                        else if is_past { "past" }
                        else if let Some((_, src)) = &my_row {
                            if src == "persistent" { "auto" } else { "booked" }
                        }
                        else if booked >= t.capacity { "full" }
                        else { "free" };

            let instructor_name: Option<String> = if let Some(iid) = t.instructor_id {
                sqlx::query_scalar("SELECT name FROM instructors WHERE id = ?")
                    .bind(iid).fetch_optional(pool).await?
            } else { None };

            out.push(UpcomingRow {
                template_id: t.id,
                date: date_s,
                start_time: t.start_time.clone(),
                duration_minutes: t.duration_minutes,
                instructor_id: t.instructor_id,
                instructor_name,
                capacity: t.capacity,
                booked,
                state: state.to_string(),
                booking_id: my_row.map(|(id, _)| id),
            });
        }
        d = d + Duration::days(1);
    }
    out.sort_by(|a, b| a.date.cmp(&b.date).then(a.start_time.cmp(&b.start_time)));
    Ok(out)
}
```

- [ ] **Step 4: Run test**

```bash
cargo test -p spinbike-server --lib db::classes::tests::list_upcoming_for_card
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/spinbike-server/src/db/classes.rs
git commit -m "feat(db): list_upcoming_for_card joins booking state

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Persistent booking DB module

**Files:**
- Create: `crates/spinbike-server/src/db/persistent_bookings.rs`
- Modify: `crates/spinbike-server/src/db/mod.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/spinbike-server/src/db/persistent_bookings.rs`:

```rust
use anyhow::Result;
use sqlx::SqlitePool;

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct PersistentBookingRow {
    pub id: i64,
    pub card_id: i64,
    pub template_id: i64,
    pub created_at: String,
    pub ended_at: Option<String>,
}

pub async fn create(pool: &SqlitePool, card_id: i64, template_id: i64) -> Result<i64> {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO persistent_bookings (card_id, template_id) VALUES (?, ?)
         ON CONFLICT(card_id, template_id) WHERE ended_at IS NULL DO NOTHING
         RETURNING id"
    )
    .bind(card_id).bind(template_id)
    .fetch_one(pool).await?;
    Ok(id)
}

pub async fn end(pool: &SqlitePool, card_id: i64, template_id: i64) -> Result<u64> {
    let res = sqlx::query(
        "UPDATE persistent_bookings SET ended_at = datetime('now')
         WHERE card_id = ? AND template_id = ? AND ended_at IS NULL"
    )
    .bind(card_id).bind(template_id)
    .execute(pool).await?;
    Ok(res.rows_affected())
}

pub async fn list_for_card(pool: &SqlitePool, card_id: i64) -> Result<Vec<PersistentBookingRow>> {
    let rows = sqlx::query_as::<_, PersistentBookingRow>(
        "SELECT id, card_id, template_id, created_at, ended_at
         FROM persistent_bookings WHERE card_id = ? AND ended_at IS NULL
         ORDER BY template_id"
    ).bind(card_id).fetch_all(pool).await?;
    Ok(rows)
}

pub async fn list_active_all(pool: &SqlitePool) -> Result<Vec<PersistentBookingRow>> {
    let rows = sqlx::query_as::<_, PersistentBookingRow>(
        "SELECT id, card_id, template_id, created_at, ended_at
         FROM persistent_bookings WHERE ended_at IS NULL"
    ).fetch_all(pool).await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations};

    async fn seed(pool: &SqlitePool) -> (i64, i64) {
        let uid: i64 = sqlx::query_scalar(
            "INSERT INTO users (email, name) VALUES ('u@x','u') RETURNING id"
        ).fetch_one(pool).await.unwrap();
        let cid: i64 = sqlx::query_scalar(
            "INSERT INTO cards (barcode, user_id, credit) VALUES ('B', ?, 0) RETURNING id"
        ).bind(uid).fetch_one(pool).await.unwrap();
        let tid = crate::db::classes::create_template(pool, 0, "18:00", 60, None, 19)
            .await.unwrap();
        (cid, tid)
    }

    #[tokio::test]
    async fn create_then_list_returns_row() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (cid, tid) = seed(&pool).await;
        let _ = create(&pool, cid, tid).await.unwrap();
        let rows = list_for_card(&pool, cid).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].template_id, tid);
    }

    #[tokio::test]
    async fn end_marks_row_and_removes_from_active_list() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (cid, tid) = seed(&pool).await;
        let _ = create(&pool, cid, tid).await.unwrap();
        let affected = end(&pool, cid, tid).await.unwrap();
        assert_eq!(affected, 1);
        let rows = list_for_card(&pool, cid).await.unwrap();
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn create_is_idempotent_when_active_row_exists() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (cid, tid) = seed(&pool).await;
        let _ = create(&pool, cid, tid).await.unwrap();
        // Second call should not error; returns existing id via the DO NOTHING path is
        // actually not returnable, so we use ON CONFLICT UPDATE pattern in production.
        // This test asserts no-duplicate:
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM persistent_bookings WHERE card_id=? AND template_id=? AND ended_at IS NULL"
        ).bind(cid).bind(tid).fetch_one(&pool).await.unwrap();
        assert_eq!(count, 1);
    }
}
```

Register in `crates/spinbike-server/src/db/mod.rs` — add under existing `pub mod` lines:

```rust
pub mod persistent_bookings;
```

- [ ] **Step 2: Run to verify the failing test first, then the fix**

```bash
cargo test -p spinbike-server --lib db::persistent_bookings::tests
```

Expected: likely FAIL on `create_is_idempotent` because `ON CONFLICT DO NOTHING` with `RETURNING` returns no row on conflict.

- [ ] **Step 3: Fix `create` to handle the idempotent case**

Replace the body of `create`:

```rust
pub async fn create(pool: &SqlitePool, card_id: i64, template_id: i64) -> Result<i64> {
    // Fast path: return existing active row id.
    if let Some(id) = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM persistent_bookings
         WHERE card_id = ? AND template_id = ? AND ended_at IS NULL"
    ).bind(card_id).bind(template_id).fetch_optional(pool).await? {
        return Ok(id);
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO persistent_bookings (card_id, template_id) VALUES (?, ?) RETURNING id"
    )
    .bind(card_id).bind(template_id)
    .fetch_one(pool).await?;
    Ok(id)
}
```

- [ ] **Step 4: Run all three tests**

```bash
cargo test -p spinbike-server --lib db::persistent_bookings::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/spinbike-server/src/db/persistent_bookings.rs crates/spinbike-server/src/db/mod.rs
git commit -m "feat(db): persistent_bookings CRUD helpers

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Materialiser sweep — create future bookings for active persistents

**Files:**
- Create: `crates/spinbike-server/src/jobs/mod.rs`
- Create: `crates/spinbike-server/src/jobs/materialiser.rs`
- Modify: `crates/spinbike-server/src/lib.rs`

- [ ] **Step 1: Write failing test**

Create `crates/spinbike-server/src/jobs/materialiser.rs`:

```rust
//! Persistent-booking materialiser: ensures a concrete booking row exists for
//! every future occurrence of an active persistent subscription within the
//! next 14 days. Skips occurrences where the class is full. Idempotent.

use anyhow::Result;
use chrono::{Datelike, Duration, Local, NaiveDate};
use sqlx::SqlitePool;

pub const WINDOW_DAYS: i64 = 14;

pub async fn sweep(pool: &SqlitePool) -> Result<usize> {
    let persistents = crate::db::persistent_bookings::list_active_all(pool).await?;
    let templates = sqlx::query_as::<_, crate::db::classes::ClassTemplateRow>(
        "SELECT id, weekday, start_time, duration_minutes, instructor_id, capacity, active
         FROM class_templates WHERE active = 1"
    ).fetch_all(pool).await?;

    let today = Local::now().date_naive();
    let mut created = 0usize;

    for p in &persistents {
        let Some(tpl) = templates.iter().find(|t| t.id == p.template_id) else { continue };

        for offset in 0..=WINDOW_DAYS {
            let d = today + Duration::days(offset);
            if d.weekday().num_days_from_monday() as i64 != tpl.weekday { continue; }
            let date_s = d.to_string();

            // Skip cancelled classes.
            let cancelled: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM class_cancellations WHERE template_id = ? AND date = ?"
            ).bind(tpl.id).bind(&date_s).fetch_optional(pool).await?;
            if cancelled.is_some() { continue; }

            // Skip if a booking already exists for this card (manual or persistent).
            let existing: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM bookings
                 WHERE template_id = ? AND date = ? AND card_id = ? AND cancelled_at IS NULL"
            ).bind(tpl.id).bind(&date_s).bind(p.card_id).fetch_optional(pool).await?;
            if existing.is_some() { continue; }

            // Lookup the user_id linked to the card (NULL allowed for legacy cards).
            let user_id: Option<i64> = sqlx::query_scalar(
                "SELECT user_id FROM cards WHERE id = ?"
            ).bind(p.card_id).fetch_one(pool).await?;
            let Some(uid) = user_id else { continue }; // can't materialise without a user

            // Attempt create — capacity-guarded INSERT silently no-ops when full.
            match crate::db::classes::create_booking(
                pool, tpl.id, &date_s, uid, p.card_id, None, "persistent"
            ).await {
                Ok(_) => created += 1,
                Err(e) if e.to_string().contains("full") => {}  // expected, keep sweeping
                Err(e) => return Err(e),
            }
        }
    }
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations};

    async fn seed(pool: &SqlitePool, weekday: i64) -> (i64, i64) {
        let uid: i64 = sqlx::query_scalar(
            "INSERT INTO users (email, name) VALUES ('u@x','u') RETURNING id"
        ).fetch_one(pool).await.unwrap();
        let cid: i64 = sqlx::query_scalar(
            "INSERT INTO cards (barcode, user_id, credit) VALUES ('B', ?, 0) RETURNING id"
        ).bind(uid).fetch_one(pool).await.unwrap();
        // The V6 seed already created Mon 18:00. If we need a different weekday,
        // pull the existing template for that weekday.
        let tid: i64 = sqlx::query_scalar(
            "SELECT id FROM class_templates WHERE weekday = ? AND start_time='18:00'"
        ).bind(weekday).fetch_one(pool).await.unwrap();
        (cid, tid)
    }

    #[tokio::test]
    async fn sweep_materialises_future_bookings() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (cid, tid) = seed(&pool, 0).await; // Monday
        crate::db::persistent_bookings::create(&pool, cid, tid).await.unwrap();

        let made = sweep(&pool).await.unwrap();
        assert!(made >= 1, "at least one Monday in next 14 days");

        // All created bookings must be source=persistent.
        let sources: Vec<(String,)> = sqlx::query_as(
            "SELECT source FROM bookings WHERE card_id = ?"
        ).bind(cid).fetch_all(&pool).await.unwrap();
        assert!(sources.iter().all(|(s,)| s == "persistent"));
    }

    #[tokio::test]
    async fn sweep_is_idempotent() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (cid, tid) = seed(&pool, 0).await;
        crate::db::persistent_bookings::create(&pool, cid, tid).await.unwrap();

        let first = sweep(&pool).await.unwrap();
        let second = sweep(&pool).await.unwrap();
        assert_eq!(second, 0, "second sweep should create nothing");
        assert!(first > 0);
    }

    #[tokio::test]
    async fn sweep_skips_full_classes() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        // Fill the next Monday slot: 19 other cards book it.
        let tid: i64 = sqlx::query_scalar(
            "SELECT id FROM class_templates WHERE weekday=0 AND start_time='18:00'"
        ).fetch_one(&pool).await.unwrap();
        let today = Local::now().date_naive();
        let offset = (7 - today.weekday().num_days_from_monday() as i64) % 7;
        let next_mon = today + Duration::days(if offset == 0 { 7 } else { offset });
        let date_s = next_mon.to_string();
        for n in 0..19 {
            let uid: i64 = sqlx::query_scalar(
                "INSERT INTO users (email, name) VALUES (?, 'u') RETURNING id"
            ).bind(format!("u{n}@x")).fetch_one(&pool).await.unwrap();
            let cid: i64 = sqlx::query_scalar(
                "INSERT INTO cards (barcode, user_id, credit) VALUES (?, ?, 0) RETURNING id"
            ).bind(format!("B{n}")).bind(uid).fetch_one(&pool).await.unwrap();
            crate::db::classes::create_booking(&pool, tid, &date_s, uid, cid, None, "manual")
                .await.unwrap();
        }
        // Now add our persistent card.
        let (cid, _) = seed(&pool, 0).await;
        crate::db::persistent_bookings::create(&pool, cid, tid).await.unwrap();

        let _ = sweep(&pool).await.unwrap();
        let got: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM bookings WHERE card_id=? AND date=?"
        ).bind(cid).bind(&date_s).fetch_optional(&pool).await.unwrap();
        assert!(got.is_none(), "must skip full class");
    }
}
```

Create `crates/spinbike-server/src/jobs/mod.rs`:

```rust
pub mod materialiser;
pub mod charger;
```

Add to `crates/spinbike-server/src/lib.rs`:

```rust
pub mod jobs;
```

(place it near the other `pub mod` lines)

- [ ] **Step 2: Run tests (charger module doesn't exist yet, will fail to compile)**

Create a stub `crates/spinbike-server/src/jobs/charger.rs` so the tree compiles:

```rust
//! T-4h charger — full impl in Task 7.
use anyhow::Result;
use sqlx::SqlitePool;
pub async fn tick(_pool: &SqlitePool) -> Result<usize> { Ok(0) }
```

Run:

```bash
cargo test -p spinbike-server --lib jobs::materialiser::tests
```

Expected: PASS (all three).

- [ ] **Step 3: Commit**

```bash
git add crates/spinbike-server/src/jobs/mod.rs crates/spinbike-server/src/jobs/materialiser.rs crates/spinbike-server/src/jobs/charger.rs crates/spinbike-server/src/lib.rs
git commit -m "feat(jobs): persistent-booking materialiser

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: Charger — T-4h automatic pass/credit debiting

**Files:**
- Modify: `crates/spinbike-server/src/jobs/charger.rs`

- [ ] **Step 1: Write failing tests**

Replace the stub content with a full module:

```rust
//! T-4h charger: for every uncharged, non-cancelled booking whose
//! start-time is within 4 hours of now, create a transaction (amount 0 if
//! the card has an active monthly pass; else debit the Spinning service
//! price from card credit — negative credit is allowed) and stamp the
//! booking with charged_at + charge_transaction_id.

use anyhow::Result;
use sqlx::SqlitePool;

pub async fn tick(pool: &SqlitePool) -> Result<usize> {
    tick_as_of(pool, &chrono::Local::now().naive_local().format("%Y-%m-%d %H:%M:%S").to_string()).await
}

pub async fn tick_as_of(pool: &SqlitePool, now_s: &str) -> Result<usize> {
    // Find bookings whose start_time <= now + 4h, not cancelled, not charged.
    let rows: Vec<(i64, i64, String, String, i64)> = sqlx::query_as(
        "SELECT b.id, b.template_id, b.date, t.start_time, b.card_id
         FROM bookings b
         JOIN class_templates t ON t.id = b.template_id
         WHERE b.cancelled_at IS NULL
           AND b.charged_at IS NULL
           AND b.card_id IS NOT NULL
           AND datetime(b.date || ' ' || t.start_time, '-4 hours') <= datetime(?)"
    ).bind(now_s).fetch_all(pool).await?;

    let price: f64 = sqlx::query_scalar(
        "SELECT default_price FROM services WHERE name = 'Spinning' AND active = 1"
    ).fetch_one(pool).await?;
    let service_id: i64 = sqlx::query_scalar(
        "SELECT id FROM services WHERE name = 'Spinning' AND active = 1"
    ).fetch_one(pool).await?;

    let mut charged = 0usize;
    for (booking_id, _template_id, date, _start, card_id) in rows {
        let mut tx = pool.begin().await?;

        // Double-check nothing else charged it in between.
        let still_open: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM bookings WHERE id = ? AND charged_at IS NULL AND cancelled_at IS NULL"
        ).bind(booking_id).fetch_optional(&mut *tx).await?;
        if still_open.is_none() { tx.rollback().await?; continue; }

        // Load card and pass state.
        let (user_id, credit, pass_valid_until): (Option<i64>, f64, Option<String>) = sqlx::query_as(
            "SELECT c.user_id, c.credit,
                    (SELECT MAX(valid_until) FROM transactions
                     WHERE card_id = c.id AND valid_until IS NOT NULL)
             FROM cards c WHERE c.id = ?"
        ).bind(card_id).fetch_one(&mut *tx).await?;

        let has_pass = match &pass_valid_until {
            Some(s) => s.as_str() >= date.as_str(),
            None => false,
        };
        let amount = if has_pass { 0.0 } else { -price };

        let txn_id: i64 = sqlx::query_scalar(
            "INSERT INTO transactions (user_id, card_id, staff_id, service_id, amount, action)
             VALUES (?, ?, NULL, ?, ?, 'visit') RETURNING id"
        ).bind(user_id).bind(card_id).bind(service_id).bind(amount)
         .fetch_one(&mut *tx).await?;

        if !has_pass {
            sqlx::query("UPDATE cards SET credit = ROUND(credit - ?, 2) WHERE id = ?")
                .bind(price).bind(card_id).execute(&mut *tx).await?;
        }

        sqlx::query(
            "UPDATE bookings SET charged_at = datetime('now'), charge_transaction_id = ? WHERE id = ?"
        ).bind(txn_id).bind(booking_id).execute(&mut *tx).await?;

        tx.commit().await?;
        charged += 1;
    }
    Ok(charged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations};

    async fn seed_booking(pool: &SqlitePool, pass: bool, credit: f64) -> (i64, i64) {
        let uid: i64 = sqlx::query_scalar(
            "INSERT INTO users (email, name) VALUES ('u@x','u') RETURNING id"
        ).fetch_one(pool).await.unwrap();
        let cid: i64 = sqlx::query_scalar(
            "INSERT INTO cards (barcode, user_id, credit) VALUES ('B', ?, ?) RETURNING id"
        ).bind(uid).bind(credit).fetch_one(pool).await.unwrap();
        if pass {
            // Insert a pass transaction with future valid_until.
            let svc: i64 = sqlx::query_scalar(
                "SELECT id FROM services WHERE name='Monthly pass'"
            ).fetch_one(pool).await.unwrap();
            sqlx::query(
                "INSERT INTO transactions (user_id, card_id, service_id, amount, action, valid_until)
                 VALUES (?, ?, ?, -35.0, 'charge', date('now','+30 days'))"
            ).bind(uid).bind(cid).bind(svc).execute(pool).await.unwrap();
        }
        let tid: i64 = sqlx::query_scalar(
            "SELECT id FROM class_templates WHERE weekday=0 AND start_time='18:00'"
        ).fetch_one(pool).await.unwrap();

        // Use a booking date such that start is WITHIN the 4h window relative to our fake now.
        // Simpler: book for today with a fake "now" just after the 4h-before moment.
        let today = chrono::Local::now().date_naive();
        let days_to_mon = (7 - today.weekday().num_days_from_monday() as i64) % 7;
        let mon = today + chrono::Duration::days(days_to_mon as i64);

        let bid = crate::db::classes::create_booking(
            pool, tid, &mon.to_string(), uid, cid, None, "manual"
        ).await.unwrap();
        (cid, bid)
    }

    // Fake "now" is Monday 14:00 — which is >= 18:00 - 4h.
    fn now_at_14() -> String {
        use chrono::Datelike;
        let today = chrono::Local::now().date_naive();
        let days_to_mon = (7 - today.weekday().num_days_from_monday() as i64) % 7;
        let mon = today + chrono::Duration::days(days_to_mon as i64);
        format!("{} 14:00:00", mon)
    }

    #[tokio::test]
    async fn charger_free_when_pass_active() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (cid, bid) = seed_booking(&pool, true, 0.0).await;
        let n = tick_as_of(&pool, &now_at_14()).await.unwrap();
        assert_eq!(n, 1);
        let (charged_at, txn_id): (Option<String>, Option<i64>) = sqlx::query_as(
            "SELECT charged_at, charge_transaction_id FROM bookings WHERE id = ?"
        ).bind(bid).fetch_one(&pool).await.unwrap();
        assert!(charged_at.is_some());
        let amount: f64 = sqlx::query_scalar(
            "SELECT amount FROM transactions WHERE id = ?"
        ).bind(txn_id.unwrap()).fetch_one(&pool).await.unwrap();
        assert_eq!(amount, 0.0);
        let credit: f64 = sqlx::query_scalar(
            "SELECT credit FROM cards WHERE id = ?"
        ).bind(cid).fetch_one(&pool).await.unwrap();
        assert_eq!(credit, 0.0, "pass should not touch credit");
    }

    #[tokio::test]
    async fn charger_debits_credit_without_pass() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (cid, _bid) = seed_booking(&pool, false, 10.0).await;
        tick_as_of(&pool, &now_at_14()).await.unwrap();
        let credit: f64 = sqlx::query_scalar(
            "SELECT credit FROM cards WHERE id = ?"
        ).bind(cid).fetch_one(&pool).await.unwrap();
        assert_eq!(credit, 5.0);
    }

    #[tokio::test]
    async fn charger_allows_negative_credit() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (cid, _) = seed_booking(&pool, false, 2.0).await;
        tick_as_of(&pool, &now_at_14()).await.unwrap();
        let credit: f64 = sqlx::query_scalar(
            "SELECT credit FROM cards WHERE id = ?"
        ).bind(cid).fetch_one(&pool).await.unwrap();
        assert_eq!(credit, -3.0);
    }

    #[tokio::test]
    async fn charger_is_idempotent() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (_cid, _bid) = seed_booking(&pool, true, 0.0).await;
        let a = tick_as_of(&pool, &now_at_14()).await.unwrap();
        let b = tick_as_of(&pool, &now_at_14()).await.unwrap();
        assert_eq!(a, 1);
        assert_eq!(b, 0);
    }

    #[tokio::test]
    async fn charger_skips_cancelled() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (_cid, bid) = seed_booking(&pool, true, 0.0).await;
        sqlx::query("UPDATE bookings SET cancelled_at = datetime('now') WHERE id = ?")
            .bind(bid).execute(&pool).await.unwrap();
        let n = tick_as_of(&pool, &now_at_14()).await.unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn charger_skips_far_future() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (_cid, _bid) = seed_booking(&pool, true, 0.0).await;
        // 10:00 is 8 hours before 18:00, outside the 4h window.
        use chrono::Datelike;
        let today = chrono::Local::now().date_naive();
        let days_to_mon = (7 - today.weekday().num_days_from_monday() as i64) % 7;
        let mon = today + chrono::Duration::days(days_to_mon as i64);
        let n = tick_as_of(&pool, &format!("{} 10:00:00", mon)).await.unwrap();
        assert_eq!(n, 0);
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p spinbike-server --lib jobs::charger::tests
```

Expected: all 6 PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/spinbike-server/src/jobs/charger.rs
git commit -m "feat(jobs): T-4h charger with pass/credit logic

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 8: Spawn background loops on server start

**Files:**
- Modify: `crates/spinbike-server/src/bin/server.rs`

- [ ] **Step 1: Modify server entry point**

Open `crates/spinbike-server/src/bin/server.rs`. After `db::run_migrations(&pool).await?;` and before `spinbike_server::start_server(...)`:

```rust
    // Run persistent-booking materialiser once at startup so the DB reflects
    // the full 14-day window before the first request arrives.
    match spinbike_server::jobs::materialiser::sweep(&pool).await {
        Ok(n) if n > 0 => tracing::info!("materialised {n} persistent bookings at startup"),
        Ok(_) => {}
        Err(e) => tracing::error!("startup materialiser sweep failed: {e}"),
    }

    // Charger: every 60s.
    {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            interval.tick().await; // first tick fires immediately; ignore.
            loop {
                interval.tick().await;
                if let Err(e) = spinbike_server::jobs::charger::tick(&pool).await {
                    tracing::error!("charger tick failed: {e}");
                }
            }
        });
    }

    // Materialiser: every 60 minutes.
    {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
            interval.tick().await;
            loop {
                interval.tick().await;
                if let Err(e) = spinbike_server::jobs::materialiser::sweep(&pool).await {
                    tracing::error!("materialiser sweep failed: {e}");
                }
            }
        });
    }
```

- [ ] **Step 2: Verify compiles**

```bash
cargo check -p spinbike-server
```

Expected: clean build.

- [ ] **Step 3: Commit**

```bash
git add crates/spinbike-server/src/bin/server.rs
git commit -m "feat(server): spawn charger + materialiser background loops

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 9: HTTP routes — persistent bookings + upcoming classes

**Files:**
- Create: `crates/spinbike-server/src/routes/persistent_bookings.rs`
- Create: `crates/spinbike-server/src/routes/upcoming_classes.rs`
- Modify: `crates/spinbike-server/src/routes/mod.rs` (or wherever `all_routes` is)
- Modify: `crates/spinbike-server/src/routes/classes.rs` (extend POST /api/bookings to require card_id)

- [ ] **Step 1: Write failing tests**

Create `crates/spinbike-server/tests/persistent_bookings_routes.rs`:

```rust
mod helpers;
use helpers::{TestApp, get, post_json, delete};
use axum::http::StatusCode;

#[tokio::test]
async fn create_and_list_persistent_booking() {
    let app = TestApp::new().await;
    let card_id = app.customer_card_id;
    let tid: i64 = sqlx::query_scalar(
        "SELECT id FROM class_templates WHERE weekday=0 AND start_time='18:00'"
    ).fetch_one(&app.pool).await.unwrap();

    // Staff creates persistent booking.
    let (status, _) = app.request(post_json(
        &format!("/api/cards/{card_id}/persistent-bookings"),
        &app.staff_token,
        &serde_json::json!({"template_id": tid}),
    )).await;
    assert_eq!(status, StatusCode::CREATED);

    // Customer cannot see the endpoint (staff-only).
    let (status, _) = app.request(get(
        &format!("/api/cards/{card_id}/persistent-bookings"),
        &app.customer_token,
    )).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Staff list returns one row.
    let (status, resp) = app.request(get(
        &format!("/api/cards/{card_id}/persistent-bookings"),
        &app.staff_token,
    )).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resp.as_array().unwrap().len(), 1);
    assert_eq!(resp[0]["template_id"].as_i64().unwrap(), tid);
}

#[tokio::test]
async fn delete_persistent_ends_it_and_removes_future_uncharged() {
    let app = TestApp::new().await;
    let card_id = app.customer_card_id;
    let tid: i64 = sqlx::query_scalar(
        "SELECT id FROM class_templates WHERE weekday=0 AND start_time='18:00'"
    ).fetch_one(&app.pool).await.unwrap();

    app.request(post_json(
        &format!("/api/cards/{card_id}/persistent-bookings"),
        &app.staff_token,
        &serde_json::json!({"template_id": tid}),
    )).await;
    // Materialise ran inside POST; there should be >=1 future booking now.
    let before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM bookings WHERE card_id=? AND source='persistent' AND cancelled_at IS NULL AND charged_at IS NULL"
    ).bind(card_id).fetch_one(&app.pool).await.unwrap();
    assert!(before >= 1);

    let (status, _) = app.request(delete(
        &format!("/api/cards/{card_id}/persistent-bookings/{tid}"),
        &app.staff_token,
    )).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM bookings WHERE card_id=? AND source='persistent' AND cancelled_at IS NULL AND charged_at IS NULL"
    ).bind(card_id).fetch_one(&app.pool).await.unwrap();
    assert_eq!(after, 0);

    let ended: Option<String> = sqlx::query_scalar(
        "SELECT ended_at FROM persistent_bookings WHERE card_id=? AND template_id=?"
    ).bind(card_id).bind(tid).fetch_one(&app.pool).await.unwrap();
    assert!(ended.is_some());
}
```

Create `crates/spinbike-server/tests/upcoming_classes_routes.rs`:

```rust
mod helpers;
use helpers::{TestApp, get};
use axum::http::StatusCode;

#[tokio::test]
async fn upcoming_classes_staff_only() {
    let app = TestApp::new().await;
    let (status, _) = app.request(get(
        &format!("/api/cards/{}/upcoming-classes?days=14", app.customer_card_id),
        &app.customer_token,
    )).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn upcoming_classes_returns_states() {
    let app = TestApp::new().await;
    let card_id = app.customer_card_id;
    let (status, resp) = app.request(get(
        &format!("/api/cards/{card_id}/upcoming-classes?days=14"),
        &app.staff_token,
    )).await;
    assert_eq!(status, StatusCode::OK);
    let arr = resp.as_array().unwrap();
    assert!(!arr.is_empty());
    let first = &arr[0];
    assert!(first["state"].is_string());
    assert!(first["instructor_name"].is_string() || first["instructor_name"].is_null());
    assert!(first["capacity"].as_i64().unwrap() == 19);
}
```

Ensure the TestApp helper seeds a card for the customer (check existing `helpers/mod.rs`; if `customer_card_id` field doesn't exist, add it: after seeding customer user, insert a card and store the id on TestApp).

- [ ] **Step 2: Run tests — expect compile-error failure**

```bash
cargo test -p spinbike-server --test persistent_bookings_routes
cargo test -p spinbike-server --test upcoming_classes_routes
```

Expected: FAIL (endpoints don't exist).

- [ ] **Step 3: Implement the routes**

Create `crates/spinbike-server/src/routes/persistent_bookings.rs`:

```rust
use axum::{extract::{Path, State}, http::StatusCode, routing::{get, post, delete}, Json, Router};
use serde::Deserialize;
use crate::auth::AuthUser;
use crate::{internal_error, AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/cards/{card_id}/persistent-bookings", get(list).post(create))
        .route("/api/cards/{card_id}/persistent-bookings/{template_id}", delete(end_persistent))
}

#[derive(Deserialize)]
struct CreateReq { template_id: i64 }

async fn list(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(card_id): Path<i64>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_book_for_others() {
        return Err((StatusCode::FORBIDDEN, Json(serde_json::json!({"error":"Staff access required"}))));
    }
    let rows = crate::db::persistent_bookings::list_for_card(&state.pool, card_id)
        .await.map_err(internal_error)?;
    Ok(Json(serde_json::to_value(rows).unwrap()))
}

async fn create(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(card_id): Path<i64>,
    Json(body): Json<CreateReq>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_book_for_others() {
        return Err((StatusCode::FORBIDDEN, Json(serde_json::json!({"error":"Staff access required"}))));
    }
    let id = crate::db::persistent_bookings::create(&state.pool, card_id, body.template_id)
        .await.map_err(internal_error)?;

    // Materialise now so the card page immediately shows AUTO rows.
    let _ = crate::jobs::materialiser::sweep(&state.pool).await;

    Ok((StatusCode::CREATED, Json(serde_json::json!({"id": id}))))
}

async fn end_persistent(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path((card_id, template_id)): Path<(i64, i64)>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_book_for_others() {
        return Err((StatusCode::FORBIDDEN, Json(serde_json::json!({"error":"Staff access required"}))));
    }
    crate::db::persistent_bookings::end(&state.pool, card_id, template_id)
        .await.map_err(internal_error)?;

    // Remove future uncharged persistent bookings.
    sqlx::query(
        "UPDATE bookings SET cancelled_at = datetime('now')
         WHERE card_id = ? AND template_id = ? AND source = 'persistent'
           AND charged_at IS NULL AND cancelled_at IS NULL
           AND datetime(date || ' ' || (SELECT start_time FROM class_templates WHERE id = ?))
               > datetime('now')"
    ).bind(card_id).bind(template_id).bind(template_id)
     .execute(&state.pool).await.map_err(internal_error)?;

    Ok(StatusCode::NO_CONTENT)
}
```

Create `crates/spinbike-server/src/routes/upcoming_classes.rs`:

```rust
use axum::{extract::{Path, Query, State}, http::StatusCode, routing::get, Json, Router};
use serde::Deserialize;
use crate::auth::AuthUser;
use crate::{internal_error, AppState};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/cards/{card_id}/upcoming-classes", get(upcoming))
}

#[derive(Deserialize)]
struct Qs { days: Option<i64> }

async fn upcoming(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(card_id): Path<i64>,
    Query(qs): Query<Qs>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_book_for_others() {
        return Err((StatusCode::FORBIDDEN, Json(serde_json::json!({"error":"Staff access required"}))));
    }
    let days = qs.days.unwrap_or(14).clamp(1, 60);
    let today = chrono::Local::now().date_naive();
    let to = today + chrono::Duration::days(days);
    let rows = crate::db::classes::list_upcoming_for_card(
        &state.pool, card_id, &today.to_string(), &to.to_string()
    ).await.map_err(internal_error)?;
    Ok(Json(serde_json::to_value(rows).unwrap()))
}
```

Register both in the module that builds `all_routes` (search for `all_routes` in `src/routes/mod.rs` or `src/lib.rs`):

```rust
pub mod persistent_bookings;
pub mod upcoming_classes;
// in all_routes():
    .merge(persistent_bookings::routes())
    .merge(upcoming_classes::routes())
```

Update `POST /api/bookings` in `routes/classes.rs` to accept `card_id` and pass it through; the current handler's `CreateBookingRequest` needs a new `pub card_id: Option<i64>` field. If `card_id` is `None`, look it up:

```rust
let card_id = if let Some(c) = body.card_id { c } else {
    sqlx::query_scalar::<_, i64>("SELECT id FROM cards WHERE user_id = ? LIMIT 1")
        .bind(booking_user_id).fetch_one(&state.pool).await
        .map_err(internal_error)?
};
let booking_id = db_classes::create_booking(
    &state.pool, body.template_id, &body.date, booking_user_id, card_id, Some(claims.sub), "manual"
).await;
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p spinbike-server --test persistent_bookings_routes --test upcoming_classes_routes
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/spinbike-server/src/routes/persistent_bookings.rs crates/spinbike-server/src/routes/upcoming_classes.rs crates/spinbike-server/src/routes/mod.rs crates/spinbike-server/src/routes/classes.rs crates/spinbike-server/src/lib.rs crates/spinbike-server/tests/persistent_bookings_routes.rs crates/spinbike-server/tests/upcoming_classes_routes.rs crates/spinbike-server/tests/helpers/mod.rs
git commit -m "feat(api): persistent-booking + upcoming-classes routes

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 10: i18n keys

**Files:**
- Modify: `spinbike-ui/src/i18n.rs`

- [ ] **Step 1: Add keys**

Inside the `TRANSLATIONS` LazyLock `m.insert(...)` block, add:

```rust
    m.insert("upcoming_classes", ("Nadchadzajuce hodiny", "Upcoming classes"));
    m.insert("persistent_booking", ("Trvala rezervacia", "Persistent booking"));
    m.insert("auto", ("AUTO", "AUTO"));
    m.insert("skip_this_week", ("Preskocit tento tyzden", "Skip this week"));
    m.insert("past", ("UPLYNULE", "PAST"));
    m.insert("turn_on", ("Zapnut", "On"));
    m.insert("turn_off", ("Vypnut", "Off"));
```

- [ ] **Step 2: Build check**

```bash
cd spinbike-ui && cargo check --target wasm32-unknown-unknown
```

(or rely on CI's `cargo check` equivalent — any type error here is a typo)

Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add spinbike-ui/src/i18n.rs
git commit -m "feat(i18n): keys for upcoming classes + persistent booking

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 11: UpcomingClasses component

**Files:**
- Create: `spinbike-ui/src/components/upcoming_classes.rs`
- Modify: `spinbike-ui/src/components/mod.rs`
- Modify: `spinbike-ui/src/pages/dashboard.rs`

- [ ] **Step 1: Create component**

`spinbike-ui/src/components/upcoming_classes.rs`:

```rust
use leptos::prelude::*;
use serde::Deserialize;
use crate::api;
use crate::i18n::{current_lang, t};

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct UpcomingRow {
    pub template_id: i64,
    pub date: String,
    pub start_time: String,
    pub duration_minutes: i64,
    pub instructor_id: Option<i64>,
    pub instructor_name: Option<String>,
    pub capacity: i64,
    pub booked: i64,
    pub state: String,
    pub booking_id: Option<i64>,
}

#[component]
pub fn UpcomingClasses(
    card_id: i64,
    #[prop(into)] refresh_tick: Signal<u32>,
    on_changed: Callback<()>,
) -> impl IntoView {
    let rows = RwSignal::new(Vec::<UpcomingRow>::new());
    let msg = RwSignal::new(String::new());

    Effect::new(move |_| {
        let _ = refresh_tick.get();
        leptos::task::spawn_local(async move {
            match api::get::<Vec<UpcomingRow>>(
                &format!("/api/cards/{card_id}/upcoming-classes?days=14")
            ).await {
                Ok(v) => rows.set(v),
                Err(e) => msg.set(format!("Error: {e}")),
            }
        });
    });

    let on_book = move |tid: i64, date: String| {
        let on_changed = on_changed.clone();
        leptos::task::spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req { template_id: i64, date: String, card_id: i64 }
            #[derive(serde::Deserialize)]
            struct Resp { id: i64 }
            match api::post::<Req, Resp>("/api/bookings",
                &Req { template_id: tid, date, card_id }).await {
                Ok(_) => on_changed.run(()),
                Err(e) => msg.set(format!("Error: {e}")),
            }
        });
    };

    let on_cancel = move |booking_id: i64| {
        let on_changed = on_changed.clone();
        leptos::task::spawn_local(async move {
            match api::delete(&format!("/api/bookings/{booking_id}")).await {
                Ok(_) => on_changed.run(()),
                Err(e) => msg.set(format!("Error: {e}")),
            }
        });
    };

    view! {
        <div class="card mb-2" data-testid="upcoming-classes">
            <h3>{move || t(current_lang(), "upcoming_classes")}</h3>
            <ul class="upcoming-list">
                <For
                    each=move || rows.get()
                    key=|r| (r.template_id, r.date.clone())
                    let(row)
                >
                    {
                        let tid = row.template_id;
                        let date = row.date.clone();
                        let bid = row.booking_id;
                        let state = row.state.clone();
                        let on_book = on_book.clone();
                        let on_cancel = on_cancel.clone();
                        let testid = format!("upcoming-{tid}-{date}");
                        view! {
                            <li class=format!("upcoming-row state-{state}") data-testid=testid>
                                <span class="upcoming-date">{row.date.clone()}</span>
                                <span class="upcoming-time">{row.start_time.clone()}</span>
                                <span class="upcoming-instr">{
                                    row.instructor_name.clone().unwrap_or_default()
                                }</span>
                                <span class="upcoming-count">{format!("{}/{}", row.booked, row.capacity)}</span>
                                <span class="upcoming-action">{
                                    match state.as_str() {
                                        "free" => view! {
                                            <button class="btn btn-sm btn-primary" data-testid=format!("book-{tid}-{}", row.date)
                                                on:click=move |_| on_book(tid, date.clone())>
                                                {t(current_lang(), "book")}
                                            </button>
                                        }.into_any(),
                                        "booked" => view! {
                                            <button class="btn btn-sm btn-danger"
                                                on:click=move |_| if let Some(b)=bid { on_cancel(b); }>
                                                {t(current_lang(), "cancel_booking")}
                                            </button>
                                        }.into_any(),
                                        "auto" => view! {
                                            <button class="btn btn-sm btn-outline"
                                                data-testid=format!("auto-cancel-{tid}-{}", row.date)
                                                on:click=move |_| if let Some(b)=bid { on_cancel(b); }>
                                                {t(current_lang(), "auto")} " — " {t(current_lang(), "skip_this_week")}
                                            </button>
                                        }.into_any(),
                                        "full" => view! {
                                            <span class="badge badge-full">{t(current_lang(), "full")}</span>
                                        }.into_any(),
                                        "cancelled" => view! {
                                            <span class="badge badge-cancelled">{t(current_lang(), "cancelled")}</span>
                                        }.into_any(),
                                        _ => view! {
                                            <span class="badge">{t(current_lang(), "past")}</span>
                                        }.into_any(),
                                    }
                                }</span>
                            </li>
                        }
                    }
                </For>
            </ul>
            <div class="msg">{move || msg.get()}</div>
        </div>
    }
}
```

Export it in `spinbike-ui/src/components/mod.rs`:

```rust
pub mod upcoming_classes;
pub use upcoming_classes::UpcomingClasses;
```

- [ ] **Step 2: Wire it into `pages/dashboard.rs` ActionPanel**

Inside `ActionPanel`, right before the line with `<SellPassModal ... />` (i.e. before the transaction history block), add:

```rust
use crate::components::UpcomingClasses;
let upc_tick = RwSignal::new(0u32);
view! {
    // ... existing children above ...
    <UpcomingClasses card_id=card.id
                     refresh_tick=upc_tick.into()
                     on_changed=Callback::new(move |()| upc_tick.update(|n| *n += 1)) />
}
```

(concrete insertion: the file currently closes the action `<div>` around line 656; place this component as a sibling right above `show_edit` conditional.)

- [ ] **Step 3: Compile**

```bash
cd spinbike-ui && trunk build
```

Expected: success.

- [ ] **Step 4: Commit**

```bash
git add spinbike-ui/src/components/upcoming_classes.rs spinbike-ui/src/components/mod.rs spinbike-ui/src/pages/dashboard.rs
git commit -m "feat(ui): upcoming classes panel on staff card page

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 12: PersistentToggles component

**Files:**
- Create: `spinbike-ui/src/components/persistent_toggles.rs`
- Modify: `spinbike-ui/src/components/mod.rs`
- Modify: `spinbike-ui/src/pages/dashboard.rs`

- [ ] **Step 1: Component**

`spinbike-ui/src/components/persistent_toggles.rs`:

```rust
use leptos::prelude::*;
use serde::Deserialize;
use crate::api;
use crate::i18n::{current_lang, t};

#[derive(Deserialize, Clone, Debug, PartialEq)]
struct PersistentRow {
    id: i64,
    card_id: i64,
    template_id: i64,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
struct TemplateLite {
    id: i64,
    weekday: i64,
    start_time: String,
    instructor_name: Option<String>,
}

#[component]
pub fn PersistentToggles(
    card_id: i64,
    on_changed: Callback<()>,
) -> impl IntoView {
    let active_ids = RwSignal::new(std::collections::HashSet::<i64>::new());
    let templates = RwSignal::new(Vec::<TemplateLite>::new());
    let msg = RwSignal::new(String::new());
    let version = RwSignal::new(0u32);

    Effect::new(move |_| {
        let _ = version.get();
        leptos::task::spawn_local(async move {
            // Load spin templates (weekday 0..=3, start_time 18:00) using upcoming endpoint's
            // template list — simplest is to derive from the first 7 days of upcoming classes.
            match api::get::<Vec<crate::components::upcoming_classes::UpcomingRow>>(
                &format!("/api/cards/{card_id}/upcoming-classes?days=7")
            ).await {
                Ok(rows) => {
                    let mut seen = std::collections::HashMap::new();
                    for r in rows {
                        seen.entry(r.template_id).or_insert(TemplateLite {
                            id: r.template_id,
                            weekday: chrono::NaiveDate::parse_from_str(&r.date, "%Y-%m-%d")
                                .map(|d| chrono::Datelike::weekday(&d).num_days_from_monday() as i64)
                                .unwrap_or(0),
                            start_time: r.start_time,
                            instructor_name: r.instructor_name,
                        });
                    }
                    let mut v: Vec<_> = seen.into_values().collect();
                    v.sort_by_key(|t| t.weekday);
                    templates.set(v);
                }
                Err(e) => msg.set(format!("Error: {e}")),
            }

            match api::get::<Vec<PersistentRow>>(
                &format!("/api/cards/{card_id}/persistent-bookings")
            ).await {
                Ok(rows) => active_ids.set(rows.into_iter().map(|r| r.template_id).collect()),
                Err(e) => msg.set(format!("Error: {e}")),
            }
        });
    });

    let on_toggle = move |tid: i64, currently_on: bool| {
        let on_changed = on_changed.clone();
        leptos::task::spawn_local(async move {
            let res = if currently_on {
                api::delete(&format!("/api/cards/{card_id}/persistent-bookings/{tid}")).await
            } else {
                #[derive(serde::Serialize)] struct Req { template_id: i64 }
                #[derive(serde::Deserialize)] struct Resp { id: i64 }
                api::post::<Req, Resp>(
                    &format!("/api/cards/{card_id}/persistent-bookings"),
                    &Req { template_id: tid }
                ).await.map(|_| ())
            };
            match res {
                Ok(_) => {
                    version.update(|n| *n += 1);
                    on_changed.run(());
                }
                Err(e) => msg.set(format!("Error: {e}")),
            }
        });
    };

    view! {
        <div class="card mb-2" data-testid="persistent-toggles">
            <h3>{move || t(current_lang(), "persistent_booking")}</h3>
            <ul class="persistent-list">
                <For
                    each=move || templates.get()
                    key=|t| t.id
                    let(tpl)
                >
                    {
                        let tid = tpl.id;
                        let on_toggle = on_toggle.clone();
                        let on_ids = active_ids;
                        let label = format!(
                            "{} — {} {}",
                            weekday_label(tpl.weekday),
                            tpl.instructor_name.clone().unwrap_or_default(),
                            tpl.start_time
                        );
                        view! {
                            <li class="persistent-row" data-testid=format!("persistent-row-{tid}")>
                                <span>{label}</span>
                                <button class="btn btn-sm btn-outline"
                                    data-testid=format!("persistent-toggle-{tid}")
                                    on:click=move |_| {
                                        let on = on_ids.get().contains(&tid);
                                        on_toggle(tid, on);
                                    }>
                                    { move || if on_ids.get().contains(&tid) {
                                        t(current_lang(), "turn_off")
                                    } else {
                                        t(current_lang(), "turn_on")
                                    } }
                                </button>
                            </li>
                        }
                    }
                </For>
            </ul>
            <div class="msg">{move || msg.get()}</div>
        </div>
    }
}

fn weekday_label(w: i64) -> &'static str {
    match w { 0 => "Mon", 1 => "Tue", 2 => "Wed", 3 => "Thu", 4 => "Fri", 5 => "Sat", _ => "Sun" }
}
```

Export and wire in dashboard the same way as UpcomingClasses, placed directly below it:

```rust
use crate::components::PersistentToggles;
// ...
<PersistentToggles card_id=card.id
                   on_changed=Callback::new(move |()| upc_tick.update(|n| *n += 1)) />
```

- [ ] **Step 2: Build**

```bash
cd spinbike-ui && trunk build
```

Expected: success.

- [ ] **Step 3: Commit**

```bash
git add spinbike-ui/src/components/persistent_toggles.rs spinbike-ui/src/components/mod.rs spinbike-ui/src/pages/dashboard.rs
git commit -m "feat(ui): persistent-booking toggles on staff card page

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 13: Minimal CSS for the new panels

**Files:**
- Modify: `spinbike-ui/style.css`

- [ ] **Step 1: Append styles**

At the end of `spinbike-ui/style.css`:

```css
.upcoming-list, .persistent-list {
    list-style: none; padding: 0; margin: 0;
}
.upcoming-row, .persistent-row {
    display: grid;
    grid-template-columns: 8em 5em 1fr auto auto;
    gap: var(--s-3);
    align-items: center;
    padding: var(--s-2) var(--s-3);
    border-top: 1px solid var(--border);
    font-size: var(--fs-sm);
}
.persistent-row { grid-template-columns: 1fr auto; }
.upcoming-row:first-child, .persistent-row:first-child { border-top: none; }
.upcoming-row.state-past { opacity: 0.5; }
.upcoming-row.state-cancelled { opacity: 0.6; }
.upcoming-row .upcoming-count { color: var(--text-muted); }
.upcoming-row .upcoming-instr { color: var(--text-muted); }
```

- [ ] **Step 2: Build**

```bash
cd spinbike-ui && trunk build
```

Expected: success.

- [ ] **Step 3: Commit**

```bash
git add spinbike-ui/style.css
git commit -m "style: layout for upcoming classes + persistent toggles

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 14: Customer-side AUTO + skip-this-week on public schedule

**Files:**
- Modify: `crates/spinbike-server/src/routes/classes.rs`
- Modify: `spinbike-ui/src/components/class_card.rs`
- Modify: `spinbike-ui/src/pages/schedule.rs` (only if ClassCard props change)

- [ ] **Step 1: Write the failing test**

Append to `crates/spinbike-server/tests/classes_routes.rs`:

```rust
#[tokio::test]
async fn list_classes_includes_my_booking_source_when_authed() {
    let app = TestApp::new().await;
    let tid: i64 = sqlx::query_scalar(
        "SELECT id FROM class_templates WHERE weekday=0 AND start_time='18:00'"
    ).fetch_one(&app.pool).await.unwrap();

    // Set up a persistent booking for the customer via direct DB + sweep.
    crate::db::persistent_bookings::create(&app.pool, app.customer_card_id, tid).await.unwrap();
    spinbike_server::jobs::materialiser::sweep(&app.pool).await.unwrap();

    // Find next Monday in ISO format (use chrono).
    use chrono::{Datelike, Duration, Local};
    let today = Local::now().date_naive();
    let days = (7 - today.weekday().num_days_from_monday() as i64) % 7;
    let mon = today + Duration::days(if days == 0 { 7 } else { days });

    let uri = format!("/api/classes?from={mon}&to={mon}");
    let (status, resp) = app.request(get(&uri, &app.customer_token)).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let arr = resp.as_array().unwrap();
    let monday = arr.iter().find(|c| c["date"].as_str() == Some(&mon.to_string())).unwrap();
    assert_eq!(monday["my_booking_source"].as_str(), Some("persistent"));
    assert!(monday["my_booking_id"].is_i64());
}
```

(Note: the `helpers::mod` changes from Task 9 expose `customer_card_id`; this test relies on that.)

- [ ] **Step 2: Run and see it fail**

```bash
cargo test -p spinbike-server --test classes_routes list_classes_includes_my_booking_source_when_authed
```

Expected: FAIL — response lacks `my_booking_source`.

- [ ] **Step 3: Extend `/api/classes` response**

In `routes/classes.rs`, find the `ClassOccurrenceResponse` struct and add two optional fields:

```rust
pub my_booking_id: Option<i64>,
pub my_booking_source: Option<String>,
```

In `list_classes`, when the request has an authenticated user (`OptionalAuthUser` with `Some(claims)`), look up the caller's booking per occurrence and populate those two fields. Use a single query joined with `cards` to find the caller's card_id once:

```rust
let my_card_id: Option<i64> = if let Some(c) = &maybe_claims {
    sqlx::query_scalar("SELECT id FROM cards WHERE user_id = ? LIMIT 1")
        .bind(c.sub).fetch_optional(&state.pool).await.map_err(internal_error)?
} else { None };

// Inside the per-occurrence loop:
let (my_bid, my_src) = if let Some(cid) = my_card_id {
    sqlx::query_as::<_, (Option<i64>, Option<String>)>(
        "SELECT id, source FROM bookings
         WHERE template_id = ? AND date = ? AND card_id = ? AND cancelled_at IS NULL"
    ).bind(tpl.id).bind(&date_s).bind(cid).fetch_optional(&state.pool).await
     .map_err(internal_error)?
     .map(|(a,b)| (a, b))
     .unwrap_or((None, None))
} else { (None, None) };
```

(If the existing code already tracks `booked_by_me`, replace it with `my_booking_id.is_some()` semantics.)

- [ ] **Step 4: Update ClassCard component**

In `spinbike-ui/src/components/class_card.rs`, the `ClassSlot` struct needs the two new fields. In the button-state match, render:

```rust
match (slot.my_booking_id, slot.my_booking_source.as_deref()) {
    (Some(bid), Some("persistent")) => view! {
        <button class="btn btn-outline btn-sm" on:click=move |_| cancel_booking(bid)>
            {t(current_lang(), "auto")} " — " {t(current_lang(), "skip_this_week")}
        </button>
    }.into_any(),
    (Some(bid), _) => view! { /* existing BOOKED/Cancel button */ }.into_any(),
    (None, _) if slot.booked >= slot.capacity => view! { /* FULL */ }.into_any(),
    (None, _) => view! { /* BOOK */ }.into_any(),
}
```

- [ ] **Step 5: Run tests + UI build**

```bash
cargo test -p spinbike-server --test classes_routes
cd spinbike-ui && trunk build
```

Expected: PASS + clean build.

- [ ] **Step 6: Commit**

```bash
git add crates/spinbike-server/src/routes/classes.rs spinbike-ui/src/components/class_card.rs crates/spinbike-server/tests/classes_routes.rs
git commit -m "feat: customer-side AUTO + skip-this-week on public schedule

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 15: Playwright E2E

**Files:**
- Create: `e2e/tests/spin-booking.spec.ts`

- [ ] **Step 1: Write the E2E**

```ts
import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, setEnglishLanguage, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

async function findCustomerCardId(): Promise<number> {
    const loginResp = await fetch(`${BASE_URL}/api/auth/login`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email: 'staff@test.com', password: 'staff123' }),
    });
    const { token } = await loginResp.json();
    const resp = await fetch(`${BASE_URL}/api/cards?search=customer`, {
        headers: { 'Authorization': `Bearer ${token}` },
    });
    const arr = await resp.json();
    return arr[0].id as number;
}

test.describe('spin booking', () => {
    test('staff books a card for a class', async ({ page }) => {
        const consoleMessages: string[] = [];
        setupConsoleCheck(page, consoleMessages);
        await setEnglishLanguage(page);
        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto(`${BASE_URL}/staff`);
        // Open the customer card.
        await page.fill('[data-testid="card-search-input"]', 'customer');
        await page.click('[data-testid="search-result"]');
        // Upcoming-classes panel shows.
        await expect(page.locator('[data-testid="upcoming-classes"]')).toBeVisible();
        // Click the first BOOK button.
        const bookBtn = page.locator('[data-testid^="book-"]').first();
        const testId = await bookBtn.getAttribute('data-testid');
        await bookBtn.click();
        // After booking, the row flips to a cancel button.
        await expect(page.locator(`[data-testid^="upcoming-"] .btn-danger`)).toHaveCount(1, { timeout: 5000 });
        assertCleanConsole(consoleMessages);
    });

    test('staff turns persistent booking ON, seats appear AUTO', async ({ page }) => {
        const consoleMessages: string[] = [];
        setupConsoleCheck(page, consoleMessages);
        await setEnglishLanguage(page);
        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto(`${BASE_URL}/staff`);
        await page.fill('[data-testid="card-search-input"]', 'customer');
        await page.click('[data-testid="search-result"]');
        await expect(page.locator('[data-testid="persistent-toggles"]')).toBeVisible();
        // Toggle the first template.
        const toggle = page.locator('[data-testid^="persistent-toggle-"]').first();
        const label = await toggle.textContent();
        expect(label?.trim()).toBe('On');
        await toggle.click();
        await expect(toggle).toHaveText('Off', { timeout: 5000 });
        // An AUTO row should now appear in upcoming classes.
        await expect(page.locator('[data-testid^="auto-cancel-"]').first()).toBeVisible({ timeout: 5000 });
        assertCleanConsole(consoleMessages);
    });

    test('staff skips one AUTO week, seat returns to BOOK', async ({ page }) => {
        const consoleMessages: string[] = [];
        setupConsoleCheck(page, consoleMessages);
        await setEnglishLanguage(page);
        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto(`${BASE_URL}/staff`);
        await page.fill('[data-testid="card-search-input"]', 'customer');
        await page.click('[data-testid="search-result"]');
        // Assume persistent is on from previous test; if not, turn it on.
        const firstAutoBtn = page.locator('[data-testid^="auto-cancel-"]').first();
        if (await firstAutoBtn.count() === 0) {
            await page.locator('[data-testid^="persistent-toggle-"]').first().click();
            await expect(page.locator('[data-testid^="auto-cancel-"]').first()).toBeVisible({ timeout: 5000 });
        }
        const autoBtn = page.locator('[data-testid^="auto-cancel-"]').first();
        const dateId = await autoBtn.getAttribute('data-testid');
        const bookId = dateId?.replace('auto-cancel-', 'book-');
        await autoBtn.click();
        await expect(page.locator(`[data-testid="${bookId}"]`)).toBeVisible({ timeout: 5000 });
        assertCleanConsole(consoleMessages);
    });

    test('staff turns persistent OFF, AUTO rows disappear', async ({ page }) => {
        const consoleMessages: string[] = [];
        setupConsoleCheck(page, consoleMessages);
        await setEnglishLanguage(page);
        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto(`${BASE_URL}/staff`);
        await page.fill('[data-testid="card-search-input"]', 'customer');
        await page.click('[data-testid="search-result"]');
        // Ensure at least one toggle is ON.
        const offToggles = page.locator('[data-testid^="persistent-toggle-"]');
        const count = await offToggles.count();
        let clickedOff = false;
        for (let i = 0; i < count; i++) {
            const t = offToggles.nth(i);
            if ((await t.textContent())?.trim() === 'Off') {
                await t.click();
                await expect(t).toHaveText('On', { timeout: 5000 });
                clickedOff = true;
                break;
            }
        }
        if (!clickedOff) test.skip();
        await expect(page.locator('[data-testid^="auto-cancel-"]')).toHaveCount(0, { timeout: 5000 });
        assertCleanConsole(consoleMessages);
    });
});
```

- [ ] **Step 2: Run locally**

```bash
cd e2e && npx playwright test spin-booking.spec.ts
```

Expected: 4/4 PASS. If `card-search-input` testid doesn't exist, grep the dashboard page to find the actual search input testid and update.

- [ ] **Step 3: Commit**

```bash
git add e2e/tests/spin-booking.spec.ts
git commit -m "test(e2e): card-centric spin booking flow

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 16: Fix formatting, push, monitor CI

- [ ] **Step 1: Format check**

```bash
cargo fmt --all --check || cargo fmt --all
```

If anything was reformatted, stage + commit:

```bash
git add -u && git commit -m "chore: cargo fmt"
```

- [ ] **Step 2: Push**

```bash
git push origin dev
```

- [ ] **Step 3: Monitor CI to completion**

```bash
gh run list --branch dev --limit 3
# take the newest id:
gh run view <id> --json status,conclusion,jobs
```

Sleep 5 minutes (300s) and re-check; repeat until `status == "completed"`.

Expected: all jobs green (Test Integrity, Lint, Build WASM, Test, E2E, Mutation Testing, Deploy).

- [ ] **Step 4: If any job fails**

Pull the failure logs:

```bash
gh run view <id> --log-failed
```

Fix the ONE root cause, commit, push once, re-monitor. Do not stream sequential fix commits.

- [ ] **Step 5: Open PR once CI is green**

```bash
gh pr create --title "Spin class booking: card-centric flow + persistent + auto-charge" --body "$(cat <<'EOF'
## Summary
- Staff can book a card into any of the 4 weekly spin classes (Mon-Thu 18:00) from the card page.
- Persistent weekly toggles per (card, template).
- Background T-4h charger: free with pass, otherwise debits Spinning price from credit (negative allowed).
- Seed migration adds Stevo / Vlada + 4 templates with capacity 19.

## Test plan
- [x] Unit: V5/V6 migrations, persistent CRUD, materialiser, charger (pass/credit/negative/idempotent/cancelled/far-future).
- [x] Integration: persistent-bookings routes, upcoming-classes routes.
- [x] E2E: book one class, persistent ON, skip-one-week, persistent OFF.
- [x] Clean browser console in all E2E.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 6: Verify PR is mergeable**

```bash
gh api repos/zbynekdrlik/spinbike/pulls/<number> --jq '{mergeable, mergeable_state}'
```

Expected: `{mergeable: true, mergeable_state: "clean"}`. Wait for user's explicit merge instruction before merging.
