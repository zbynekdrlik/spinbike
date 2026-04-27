//! In-place legacy backfill: walk the `.mdb` `Data` table and set
//! transactions.service_id where currently NULL, matching prod rows by
//! (barcode, created_at, amount). Idempotent — the `service_id IS NULL`
//! guard means re-runs are safe and post-import sales are never touched.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use sqlx::SqlitePool;
use tracing::{info, warn};

/// Rows per intermediate `tx.commit()` during the backfill loop. SQLite is
/// single-writer; once an UPDATE within the tx fires, the writer lock is
/// held until COMMIT. Each apply_legacy_row issues an UPDATE as its first
/// SQL, so the writer lock is acquired upfront for the batch.
///
/// 100 was chosen empirically after the first prod run hit
/// `SQLITE_BUSY (database is locked)` on concurrent staff API calls — the
/// previous 1000-row batches in debug build held the writer lock for ~13 s,
/// well over sqlx's 5 s busy_timeout. With 100-row batches AND a release
/// build, each batch's writer-lock window is ~100 ms, comfortably under
/// the busy_timeout so concurrent writes wait briefly and succeed.
///
/// In addition, `db::create_pool` raises busy_timeout to 30 s so any future
/// admin write that briefly exceeds 5 s does not surface as a 500 to the
/// staff client.
const BACKFILL_BATCH_SIZE: u32 = 100;

#[derive(Debug, Default)]
pub struct BackfillReport {
    pub matched: u32,
    pub already_set: u32,
    pub unmatched: u32,
    pub orphan_card: u32,
    pub unknown_service: u32,
    /// Legacy rows whose date column couldn't be parsed as
    /// `MM/DD/YY HH:MM:SS`. Distinct from `unmatched` so the operator can
    /// tell "format quirk" from "no prod row matched".
    pub malformed_date: u32,
    pub ambiguous: u32,
    pub per_service: HashMap<String, ServiceCounts>,
}

#[derive(Debug, Default)]
pub struct ServiceCounts {
    pub matched: u32,
    pub already_set: u32,
    pub unmatched: u32,
    pub ambiguous: u32,
}

/// One parsed legacy `Data` row, ready for `apply_legacy_row` to dispatch.
/// Owning struct so callers (real and test) can assemble fields without
/// juggling 5+ borrows; lifetimes stay simple.
#[derive(Debug, Clone)]
pub(crate) struct LegacyRow {
    pub(crate) card_id: String,
    pub(crate) action: String,
    pub(crate) service: String,
    /// Raw legacy date string (`MM/DD/YY HH:MM:SS`). Converted to ISO
    /// before the SQL match.
    pub(crate) date: String,
    pub(crate) amount_eur: f64,
}

/// True if the legacy action represents a transaction that had a service
/// in the old data model. Top-ups, activations, and blocks legitimately
/// have no service and must be skipped during backfill.
pub(crate) fn legacy_action_has_service(action: &str) -> bool {
    !matches!(
        action.trim().trim_matches('"'),
        "Novy kredit" | "Kredit" | "AKTIVACIA" | "BLOKOVANA"
    )
}

/// Spawn `mdb-export <table>` and return its stdout as a String. Pure I/O
/// shim around an external binary — testing every mutation here would
/// require a real .mdb fixture and mdbtools installed in the test
/// environment. Excluded from mutation testing on purpose.
#[mutants::skip]
fn export_table(mdb_path: &Path, table: &str) -> Result<String> {
    let output = Command::new("mdb-export")
        .arg(mdb_path)
        .arg(table)
        .output()
        .with_context(|| format!("Failed to run mdb-export for table '{table}'"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("mdb-export failed for table '{table}': {stderr}");
    }
    String::from_utf8(output.stdout)
        .with_context(|| format!("mdb-export output for '{table}' is not valid UTF-8"))
}

/// Convert a legacy date string (`MM/DD/YY HH:MM:SS`, e.g. `11/06/08 21:17:38`)
/// to the ISO format SQLite stored on import (`YYYY-MM-DD HH:MM:SS`, e.g.
/// `2008-11-06 21:17:38`). Returns `None` for blank or unparsable input.
///
/// This is critical because legacy MDB raw strings DO NOT match prod's
/// stored format — using them verbatim in the WHERE clause silently misses
/// every row.
pub(crate) fn legacy_date_to_iso(s: &str) -> Option<String> {
    let trimmed = s.trim().trim_matches('"');
    if trimmed.is_empty() {
        return None;
    }
    chrono::NaiveDateTime::parse_from_str(trimmed, "%m/%d/%y %H:%M:%S")
        .ok()
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
}

/// Map a legacy Slovak service name to the target `name_sk` value.
/// Mirror of the migrator's mapping; defined here so the backfill module
/// is self-contained and the migrator's bin can stay focused on import.
pub fn map_legacy_service_name(name: &str) -> Option<&'static str> {
    match name.trim().trim_matches('"') {
        "Casova karta" => Some("Mesačný preplatok"),
        "Fitnes" => Some("Fitness"),
        "Spinbike" => Some("Spinning"),
        "Doplnky Vyzivy" => Some("Doplnky výživy"),
        "Obcerstvenie" => Some("Občerstvenie"),
        "AktivaciaKarty" => Some("Aktivácia karty"),
        _ => None,
    }
}

/// Apply backfill logic for one parsed legacy `Data` row.
///
/// Updates `report` according to the row's outcome (orphan card, unknown
/// service, matched, ambiguous, already_set, unmatched, malformed_date).
/// Pulled out of `run()` so it can be exercised by unit tests without
/// invoking `mdb-export`.
pub(crate) async fn apply_legacy_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    service_ids: &HashMap<String, i64>,
    legacy_card_to_barcode: &HashMap<String, String>,
    row: &LegacyRow,
    report: &mut BackfillReport,
) -> Result<()> {
    if !legacy_action_has_service(&row.action) {
        return Ok(());
    }
    if row.service.is_empty() {
        return Ok(());
    }

    let barcode = match legacy_card_to_barcode.get(&row.card_id) {
        Some(bc) => bc,
        None => {
            report.orphan_card += 1;
            return Ok(());
        }
    };

    let new_name = match map_legacy_service_name(&row.service) {
        Some(n) => n,
        None => {
            warn!(
                "unknown legacy service '{}' on row card={} \
                 (will be skipped — extend map_legacy_service_name if this should be backfilled)",
                row.service, row.card_id
            );
            report.unknown_service += 1;
            return Ok(());
        }
    };
    let svc_id = match service_ids.get(new_name) {
        Some(id) => *id,
        None => {
            warn!(
                "target DB has no service named '{new_name}' (legacy '{}'); \
                 run V8/V9 migrations first",
                row.service
            );
            report.unknown_service += 1;
            return Ok(());
        }
    };

    // Match (barcode, created_at_iso, amount with epsilon) and only touch
    // rows that don't already have a service_id.
    //
    // CONCURRENCY: this UPDATE is the FIRST SQL in apply_legacy_row, so the
    // batch transaction (DEFERRED) acquires the writer lock here on first
    // call. There is no read-snapshot to invalidate, which avoids the
    // SQLITE_BUSY_SNAPSHOT (code 517) race a SELECT-first design would
    // introduce. Combined with release-build speed and small batches
    // (BACKFILL_BATCH_SIZE = 100), the writer lock is held for ~100 ms per
    // batch — well under sqlx's 5 s busy_timeout for concurrent staff API
    // calls on the same DB.
    //
    // DATE FORMAT: the legacy MDB stores `MM/DD/YY HH:MM:SS`; the prod
    // DB stores ISO `YYYY-MM-DD HH:MM:SS` (the original importer
    // normalised them). Convert before binding.
    //
    // SIGN CONVENTION: the migrator binds `amount_eur` with the same
    // sign it has in the legacy `suma` column (positive for debits, no
    // negation). Imported transactions in prod therefore have POSITIVE
    // amounts for debits, in contrast to NEW sales which the API stores
    // as negative. The `service_id IS NULL` guard ensures we only touch
    // imported rows, so `t.amount` is always the legacy-positive value;
    // we compare with `t.amount - legacy_amount`.
    let iso_date = match legacy_date_to_iso(&row.date) {
        Some(s) => s,
        None => {
            // Unparsable legacy timestamp — distinct from "no prod row
            // matched" so the operator can tell format-quirk apart from
            // missing-data when reviewing the summary.
            warn!(
                "unparsable legacy date '{}' on row card={} service='{}'",
                row.date, row.card_id, row.service
            );
            report.malformed_date += 1;
            return Ok(());
        }
    };
    let updated_ids: Vec<(i64,)> = sqlx::query_as(
        "UPDATE transactions
            SET service_id = ?, legacy_backfilled = 1
          WHERE id IN (
            SELECT t.id
              FROM transactions t
              JOIN cards c ON c.id = t.card_id
             WHERE c.barcode = ?
               AND t.created_at = ?
               AND ABS(t.amount - ?) < 0.005
               AND t.service_id IS NULL
          )
          RETURNING id",
    )
    .bind(svc_id)
    .bind(barcode)
    .bind(&iso_date)
    .bind(row.amount_eur)
    .fetch_all(&mut **tx)
    .await
    .context("backfill UPDATE failed")?;

    let bucket = report.per_service.entry(new_name.to_string()).or_default();
    match updated_ids.len() {
        0 => {
            // Either this legacy row's prod equivalent already has a
            // service_id (= already-set on a prior backfill or a
            // post-import sale), or there's no matching prod row at all.
            let exists: Option<i64> = sqlx::query_scalar(
                "SELECT t.id FROM transactions t
                   JOIN cards c ON c.id = t.card_id
                  WHERE c.barcode = ? AND t.created_at = ?
                    AND ABS(t.amount - ?) < 0.005
                  LIMIT 1",
            )
            .bind(barcode)
            .bind(&iso_date)
            .bind(row.amount_eur)
            .fetch_optional(&mut **tx)
            .await
            .context("ambiguity probe failed")?;
            if exists.is_some() {
                report.already_set += 1;
                bucket.already_set += 1;
            } else {
                report.unmatched += 1;
                bucket.unmatched += 1;
            }
        }
        1 => {
            report.matched += 1;
            bucket.matched += 1;
        }
        n => {
            report.matched += n as u32;
            bucket.matched += n as u32;
            report.ambiguous += 1;
            bucket.ambiguous += 1;
            warn!(
                "ambiguous: legacy row card={} date={} amount={} matched {n} prod rows: {:?}",
                row.card_id,
                row.date,
                row.amount_eur,
                updated_ids.iter().map(|(i,)| *i).collect::<Vec<_>>()
            );
        }
    }
    Ok(())
}

/// Run the in-place backfill against `pool`, reading legacy data from `mdb_path`.
///
/// Idempotent: only updates rows where `service_id IS NULL`. Sets
/// `legacy_backfilled = 1` alongside `service_id` so a targeted rollback
/// is possible.
///
/// The per-row decision logic lives in `apply_legacy_row` (mutation-tested
/// independently). This function is the orchestration shell — file I/O,
/// CSV iteration, batched commits — and depends on `mdb-export` being
/// installed; mutating it would require an end-to-end .mdb harness rather
/// than meaningful unit-level coverage. Excluded from mutation testing.
#[mutants::skip]
pub async fn run(pool: &SqlitePool, mdb_path: &Path) -> Result<BackfillReport> {
    info!("Loading services from target DB...");
    let service_ids: HashMap<String, i64> =
        sqlx::query_as::<_, (String, i64)>("SELECT name_sk, id FROM services")
            .fetch_all(pool)
            .await
            .context("Failed to load services from target")?
            .into_iter()
            .collect();

    info!("Reading legacy card table from {}", mdb_path.display());
    let card_csv = export_table(mdb_path, "card")?;
    let mut card_reader = csv::Reader::from_reader(Cursor::new(&card_csv));
    let mut legacy_card_to_barcode: HashMap<String, String> = HashMap::new();
    for result in card_reader.records() {
        let r = result.context("parse legacy card row")?;
        let id = r.get(0).unwrap_or("").trim().to_string();
        let barcode = r.get(1).unwrap_or("").trim().to_string();
        if !id.is_empty() && !barcode.is_empty() {
            legacy_card_to_barcode.insert(id, barcode);
        }
    }
    info!(
        "Mapped {} legacy cards to barcodes",
        legacy_card_to_barcode.len()
    );

    info!("Reading legacy Data table...");
    let data_csv = export_table(mdb_path, "Data")?;
    let mut data_reader = csv::Reader::from_reader(Cursor::new(&data_csv));

    let mut report = BackfillReport::default();

    // Batch row UPDATEs into BACKFILL_BATCH_SIZE-row transactions instead of
    // one big tx for the whole run. SQLite is single-writer, and a single
    // ~3-minute tx would queue all concurrent staff actions (charges,
    // top-ups, sell-pass) for the duration. Committing every N rows
    // briefly releases the writer lock (sub-second window) so concurrent
    // writes can interleave.
    //
    // Idempotency is preserved by the `service_id IS NULL` guard inside
    // apply_legacy_row's UPDATE — a half-run that crashes mid-batch leaves
    // committed rows in the right state, and a re-run skips them. A failure
    // mid-batch rolls back only that batch, so the worst case is ~N rows
    // re-processed on the next run.
    let mut tx = pool.begin().await.context("Failed to begin backfill tx")?;
    let mut rows_in_batch = 0u32;

    for result in data_reader.records() {
        let r = result.context("parse legacy Data row")?;
        // Header: id_data,id_card,user,action,service,suma_SK,Date,EndDate,suma
        let row = LegacyRow {
            card_id: r.get(1).unwrap_or("").trim().to_string(),
            action: r.get(3).unwrap_or("").trim().to_string(),
            service: r.get(4).unwrap_or("").trim().trim_matches('"').to_string(),
            date: r.get(6).unwrap_or("").trim().to_string(),
            amount_eur: r.get(8).unwrap_or("0").trim().parse().unwrap_or(0.0),
        };

        apply_legacy_row(
            &mut tx,
            &service_ids,
            &legacy_card_to_barcode,
            &row,
            &mut report,
        )
        .await?;

        rows_in_batch += 1;
        if rows_in_batch >= BACKFILL_BATCH_SIZE {
            tx.commit()
                .await
                .context("Failed to commit backfill batch")?;
            tx = pool
                .begin()
                .await
                .context("Failed to begin next backfill batch")?;
            rows_in_batch = 0;
        }
    }

    tx.commit()
        .await
        .context("Failed to commit final backfill batch")?;

    info!("=== Backfill summary ===");
    let mut services: Vec<&String> = report.per_service.keys().collect();
    services.sort();
    for svc in services {
        let c = &report.per_service[svc];
        info!(
            "  {svc}: matched={} already-set={} unmatched={} ambiguous={}",
            c.matched, c.already_set, c.unmatched, c.ambiguous
        );
    }
    info!(
        "  TOTAL: matched={} already-set={} unmatched={} ambiguous={} orphan_card={} unknown_service={} malformed_date={}",
        report.matched,
        report.already_set,
        report.unmatched,
        report.ambiguous,
        report.orphan_card,
        report.unknown_service,
        report.malformed_date
    );

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_memory_pool, run_migrations};
    use sqlx::SqlitePool;

    async fn doplnky_service_id(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT id FROM services WHERE name_sk = 'Doplnky výživy'")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// Helper that runs the same UPDATE the public `run()` issues for one
    /// (barcode, date, amount) tuple. Returns the prod ids that were updated.
    async fn backfill_one(
        pool: &SqlitePool,
        svc_id: i64,
        barcode: &str,
        date: &str,
        amount_eur: f64,
    ) -> Vec<i64> {
        let rows: Vec<(i64,)> = sqlx::query_as(
            "UPDATE transactions
                SET service_id = ?, legacy_backfilled = 1
              WHERE id IN (
                SELECT t.id FROM transactions t
                  JOIN cards c ON c.id = t.card_id
                 WHERE c.barcode = ? AND t.created_at = ?
                   AND ABS(t.amount - ?) < 0.005
                   AND t.service_id IS NULL
              ) RETURNING id",
        )
        .bind(svc_id)
        .bind(barcode)
        .bind(date)
        .bind(amount_eur)
        .fetch_all(pool)
        .await
        .unwrap();
        rows.into_iter().map(|(i,)| i).collect()
    }

    #[tokio::test]
    async fn backfill_idempotent_first_run_matches_second_does_nothing() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let card_id: i64 = sqlx::query_scalar(
            "INSERT INTO cards (barcode, allow_debit) VALUES ('LEG-1', 1) RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO transactions (card_id, amount, action, created_at)
             VALUES (?, 1.66, 'debit', '2008-11-06 21:31:04')",
        )
        .bind(card_id)
        .execute(&pool)
        .await
        .unwrap();
        let svc_id = doplnky_service_id(&pool).await;

        let first = backfill_one(&pool, svc_id, "LEG-1", "2008-11-06 21:31:04", 1.66).await;
        assert_eq!(first.len(), 1, "first run should match the row");

        let second = backfill_one(&pool, svc_id, "LEG-1", "2008-11-06 21:31:04", 1.66).await;
        assert_eq!(second.len(), 0, "second run must not match (NULL guard)");

        let svc: Option<i64> =
            sqlx::query_scalar("SELECT service_id FROM transactions WHERE card_id = ?")
                .bind(card_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(svc, Some(svc_id));

        // legacy_backfilled marker set on the matched row.
        let backfilled: i64 =
            sqlx::query_scalar("SELECT legacy_backfilled FROM transactions WHERE card_id = ?")
                .bind(card_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(backfilled, 1);
    }

    #[tokio::test]
    async fn backfill_skips_post_import_sales_with_existing_service() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let card_id: i64 = sqlx::query_scalar(
            "INSERT INTO cards (barcode, allow_debit) VALUES ('LEG-2', 1) RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let fitness_id: i64 =
            sqlx::query_scalar("SELECT id FROM services WHERE name_sk = 'Fitness'")
                .fetch_one(&pool)
                .await
                .unwrap();
        sqlx::query(
            "INSERT INTO transactions (card_id, service_id, amount, action, created_at)
             VALUES (?, ?, 1.66, 'debit', '2008-11-06 21:31:04')",
        )
        .bind(card_id)
        .bind(fitness_id)
        .execute(&pool)
        .await
        .unwrap();
        let svc_id = doplnky_service_id(&pool).await;

        let updated = backfill_one(&pool, svc_id, "LEG-2", "2008-11-06 21:31:04", 1.66).await;
        assert_eq!(
            updated.len(),
            0,
            "row already has service_id; must not be touched"
        );

        let svc_after: Option<i64> =
            sqlx::query_scalar("SELECT service_id FROM transactions WHERE card_id = ?")
                .bind(card_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            svc_after,
            Some(fitness_id),
            "service_id must remain Fitness"
        );

        // legacy_backfilled stays 0 on a row the backfill didn't touch.
        let backfilled: i64 =
            sqlx::query_scalar("SELECT legacy_backfilled FROM transactions WHERE card_id = ?")
                .bind(card_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(backfilled, 0);
    }

    #[tokio::test]
    async fn backfill_ambiguous_match_updates_all() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let card_id: i64 = sqlx::query_scalar(
            "INSERT INTO cards (barcode, allow_debit) VALUES ('LEG-3', 1) RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO transactions (card_id, amount, action, created_at)
             VALUES (?, 1.66, 'debit', '2008-11-06 21:31:04'),
                    (?, 1.66, 'debit', '2008-11-06 21:31:04')",
        )
        .bind(card_id)
        .bind(card_id)
        .execute(&pool)
        .await
        .unwrap();
        let svc_id = doplnky_service_id(&pool).await;

        let updated = backfill_one(&pool, svc_id, "LEG-3", "2008-11-06 21:31:04", 1.66).await;
        assert_eq!(
            updated.len(),
            2,
            "ambiguous: both rows updated to same service_id"
        );
    }

    #[test]
    fn legacy_action_has_service_excludes_topups_and_blocks() {
        assert!(!legacy_action_has_service("Novy kredit"));
        assert!(!legacy_action_has_service("\"Novy kredit\""));
        assert!(!legacy_action_has_service("AKTIVACIA"));
        assert!(!legacy_action_has_service("BLOKOVANA"));
        assert!(legacy_action_has_service("Debet"));
        assert!(legacy_action_has_service("Storno"));
    }

    #[test]
    fn legacy_date_to_iso_converts_known_format() {
        // Real prod data shape: legacy `MM/DD/YY HH:MM:SS` -> ISO.
        assert_eq!(
            legacy_date_to_iso("11/06/08 21:17:38").as_deref(),
            Some("2008-11-06 21:17:38")
        );
        assert_eq!(
            legacy_date_to_iso("12/31/25 13:39:05").as_deref(),
            Some("2025-12-31 13:39:05")
        );
        // Quoted CSV cell — strip quotes before parsing.
        assert_eq!(
            legacy_date_to_iso("\"11/06/08 21:17:38\"").as_deref(),
            Some("2008-11-06 21:17:38")
        );
    }

    #[test]
    fn legacy_date_to_iso_rejects_unparseable_inputs() {
        assert_eq!(legacy_date_to_iso(""), None);
        assert_eq!(legacy_date_to_iso("   "), None);
        assert_eq!(legacy_date_to_iso("not a date"), None);
        // Already ISO — not the legacy format, so refuse rather than guess.
        assert_eq!(legacy_date_to_iso("2008-11-06 21:17:38"), None);
    }

    #[test]
    fn map_legacy_service_name_covers_all_six() {
        assert_eq!(map_legacy_service_name("Fitnes"), Some("Fitness"));
        assert_eq!(map_legacy_service_name("Spinbike"), Some("Spinning"));
        assert_eq!(
            map_legacy_service_name("Casova karta"),
            Some("Mesačný preplatok")
        );
        assert_eq!(
            map_legacy_service_name("Doplnky Vyzivy"),
            Some("Doplnky výživy")
        );
        assert_eq!(
            map_legacy_service_name("Obcerstvenie"),
            Some("Občerstvenie")
        );
        assert_eq!(
            map_legacy_service_name("AktivaciaKarty"),
            Some("Aktivácia karty")
        );
        assert_eq!(map_legacy_service_name("Storno"), None);
        assert_eq!(map_legacy_service_name("Iont"), None);
    }

    // ----- apply_legacy_row branch coverage -----
    //
    // These tests exercise every counter increment in apply_legacy_row to
    // catch mutation-testing survivors (replace +=1 with -=1 / *=1, delete !,
    // etc.). Each test asserts the EXACT report state after one row, so any
    // mutated arithmetic produces a different value and gets caught.

    /// Build a (service_ids, legacy_card_to_barcode) tuple from a fresh DB.
    /// Inserts a card with barcode "BC-1" and id 1, returns the maps.
    async fn fixture_state(pool: &SqlitePool) -> (HashMap<String, i64>, HashMap<String, String>) {
        sqlx::query("INSERT INTO cards (barcode, allow_debit) VALUES ('BC-1', 1)")
            .execute(pool)
            .await
            .unwrap();
        let svc: HashMap<String, i64> =
            sqlx::query_as::<_, (String, i64)>("SELECT name_sk, id FROM services")
                .fetch_all(pool)
                .await
                .unwrap()
                .into_iter()
                .collect();
        let cards: HashMap<String, String> =
            HashMap::from([("100".to_string(), "BC-1".to_string())]);
        (svc, cards)
    }

    /// Tiny constructor so each test can express its row as a one-liner.
    fn legacy(
        card_id: &str,
        action: &str,
        service: &str,
        date: &str,
        amount_eur: f64,
    ) -> LegacyRow {
        LegacyRow {
            card_id: card_id.into(),
            action: action.into(),
            service: service.into(),
            date: date.into(),
            amount_eur,
        }
    }

    #[tokio::test]
    async fn apply_row_orphan_card_increments_only_orphan() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (svc, cards) = fixture_state(&pool).await;
        let mut tx = pool.begin().await.unwrap();
        let mut report = BackfillReport::default();

        apply_legacy_row(
            &mut tx,
            &svc,
            &cards,
            // 9999 is not in cards map
            &legacy("9999", "Debet", "Doplnky Vyzivy", "11/06/08 12:00:00", 1.66),
            &mut report,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        assert_eq!(report.orphan_card, 1);
        assert_eq!(report.matched, 0);
        assert_eq!(report.already_set, 0);
        assert_eq!(report.unmatched, 0);
        assert_eq!(report.ambiguous, 0);
        assert_eq!(report.unknown_service, 0);
        assert_eq!(report.malformed_date, 0);
    }

    #[tokio::test]
    async fn apply_row_unknown_legacy_service_increments_unknown() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (svc, cards) = fixture_state(&pool).await;
        let mut tx = pool.begin().await.unwrap();
        let mut report = BackfillReport::default();

        apply_legacy_row(
            &mut tx,
            &svc,
            &cards,
            // "Iont" is not in map_legacy_service_name
            &legacy("100", "Debet", "Iont", "11/06/08 12:00:00", 1.0),
            &mut report,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        assert_eq!(report.unknown_service, 1);
        assert_eq!(report.orphan_card, 0);
        assert_eq!(report.matched, 0);
    }

    #[tokio::test]
    async fn apply_row_unknown_target_service_increments_unknown() {
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (mut svc, cards) = fixture_state(&pool).await;
        // Force the "target DB has no service named X" branch by removing the
        // mapped entry from the snapshot the function reads.
        svc.remove("Doplnky výživy");
        let mut tx = pool.begin().await.unwrap();
        let mut report = BackfillReport::default();

        apply_legacy_row(
            &mut tx,
            &svc,
            &cards,
            &legacy("100", "Debet", "Doplnky Vyzivy", "11/06/08 12:00:00", 1.66),
            &mut report,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        assert_eq!(report.unknown_service, 1);
        assert_eq!(report.matched, 0);
    }

    #[tokio::test]
    async fn apply_row_skips_topup_action_with_no_counter_change() {
        // legacy_action_has_service('Novy kredit') == false -> no counter touched.
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (svc, cards) = fixture_state(&pool).await;
        let mut tx = pool.begin().await.unwrap();
        let mut report = BackfillReport::default();

        apply_legacy_row(
            &mut tx,
            &svc,
            &cards,
            &legacy("100", "Novy kredit", "", "11/06/08 12:00:00", 10.0),
            &mut report,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        assert_eq!(report.orphan_card, 0);
        assert_eq!(report.unknown_service, 0);
        assert_eq!(report.matched, 0);
        assert_eq!(report.already_set, 0);
        assert_eq!(report.unmatched, 0);
        assert_eq!(report.ambiguous, 0);
        assert_eq!(report.malformed_date, 0);
    }

    #[tokio::test]
    async fn apply_row_unmatched_increments_unmatched_per_service() {
        // No prod transaction matches -> unmatched += 1, bucket.unmatched += 1.
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (svc, cards) = fixture_state(&pool).await;
        let mut tx = pool.begin().await.unwrap();
        let mut report = BackfillReport::default();

        apply_legacy_row(
            &mut tx,
            &svc,
            &cards,
            &legacy("100", "Debet", "Doplnky Vyzivy", "11/06/08 12:00:00", 1.66),
            &mut report,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        assert_eq!(report.unmatched, 1);
        assert_eq!(report.matched, 0);
        assert_eq!(report.already_set, 0);
        assert_eq!(report.ambiguous, 0);
        let bucket = report.per_service.get("Doplnky výživy").unwrap();
        assert_eq!(bucket.unmatched, 1);
        assert_eq!(bucket.matched, 0);
    }

    #[tokio::test]
    async fn apply_row_unparsable_date_increments_malformed_date() {
        // Unparsable legacy date -> malformed_date += 1 (NOT unmatched, so the
        // operator can distinguish "format quirk" from "no prod row matched").
        // The function returns before the UPDATE so per-service buckets are
        // never touched on this branch.
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (svc, cards) = fixture_state(&pool).await;
        let mut tx = pool.begin().await.unwrap();
        let mut report = BackfillReport::default();

        apply_legacy_row(
            &mut tx,
            &svc,
            &cards,
            &legacy(
                "100",
                "Debet",
                "Doplnky Vyzivy",
                "this is not a legacy date",
                1.66,
            ),
            &mut report,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        assert_eq!(
            report.malformed_date, 1,
            "unparsable date increments malformed_date"
        );
        assert_eq!(report.unmatched, 0, "must NOT spill into unmatched");
        assert_eq!(report.matched, 0);
        assert_eq!(report.already_set, 0);
        assert_eq!(report.ambiguous, 0);
        assert_eq!(report.orphan_card, 0);
        assert_eq!(report.unknown_service, 0);
        assert!(
            report.per_service.is_empty(),
            "per-service buckets are not touched by the malformed_date branch"
        );
    }

    #[tokio::test]
    async fn apply_row_matched_one_increments_matched_per_service() {
        // One NULL-service prod txn matches -> matched += 1, bucket.matched += 1.
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (svc, cards) = fixture_state(&pool).await;
        sqlx::query(
            "INSERT INTO transactions (card_id, amount, action, created_at)
             VALUES (1, 1.66, 'debit', '2008-11-06 12:00:00')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        let mut report = BackfillReport::default();

        apply_legacy_row(
            &mut tx,
            &svc,
            &cards,
            &legacy("100", "Debet", "Doplnky Vyzivy", "11/06/08 12:00:00", 1.66),
            &mut report,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        assert_eq!(report.matched, 1);
        assert_eq!(report.already_set, 0);
        assert_eq!(report.unmatched, 0);
        assert_eq!(report.ambiguous, 0);
        let bucket = report.per_service.get("Doplnky výživy").unwrap();
        assert_eq!(bucket.matched, 1);
    }

    #[tokio::test]
    async fn apply_row_already_set_increments_already_set() {
        // Prod txn already has a service_id -> already_set += 1.
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (svc, cards) = fixture_state(&pool).await;
        let fitness_id: i64 =
            sqlx::query_scalar("SELECT id FROM services WHERE name_sk = 'Fitness'")
                .fetch_one(&pool)
                .await
                .unwrap();
        sqlx::query(
            "INSERT INTO transactions (card_id, service_id, amount, action, created_at)
             VALUES (1, ?, 1.66, 'debit', '2008-11-06 12:00:00')",
        )
        .bind(fitness_id)
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        let mut report = BackfillReport::default();

        apply_legacy_row(
            &mut tx,
            &svc,
            &cards,
            &legacy("100", "Debet", "Doplnky Vyzivy", "11/06/08 12:00:00", 1.66),
            &mut report,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        assert_eq!(report.already_set, 1);
        assert_eq!(report.matched, 0);
        assert_eq!(report.unmatched, 0);
        assert_eq!(report.ambiguous, 0);
        let bucket = report.per_service.get("Doplnky výživy").unwrap();
        assert_eq!(bucket.already_set, 1);
    }

    #[tokio::test]
    async fn apply_row_ambiguous_match_increments_correctly() {
        // Two NULL-service prod txns share the same key -> matched += 2,
        // ambiguous += 1.
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();
        let (svc, cards) = fixture_state(&pool).await;
        sqlx::query(
            "INSERT INTO transactions (card_id, amount, action, created_at)
             VALUES (1, 1.66, 'debit', '2008-11-06 12:00:00'),
                    (1, 1.66, 'debit', '2008-11-06 12:00:00')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        let mut report = BackfillReport::default();

        apply_legacy_row(
            &mut tx,
            &svc,
            &cards,
            &legacy("100", "Debet", "Doplnky Vyzivy", "11/06/08 12:00:00", 1.66),
            &mut report,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        assert_eq!(report.matched, 2, "both rows count toward matched");
        assert_eq!(report.ambiguous, 1, "one ambiguous event");
        assert_eq!(report.already_set, 0);
        assert_eq!(report.unmatched, 0);
        let bucket = report.per_service.get("Doplnky výživy").unwrap();
        assert_eq!(bucket.matched, 2);
        assert_eq!(bucket.ambiguous, 1);
    }
}
