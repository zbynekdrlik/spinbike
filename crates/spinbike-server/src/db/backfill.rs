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

#[derive(Debug, Default)]
pub struct BackfillReport {
    pub matched: u32,
    pub already_set: u32,
    pub unmatched: u32,
    pub orphan_card: u32,
    pub unknown_service: u32,
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

/// True if the legacy action represents a transaction that had a service
/// in the old data model. Top-ups, activations, and blocks legitimately
/// have no service and must be skipped during backfill.
pub(crate) fn legacy_action_has_service(action: &str) -> bool {
    !matches!(
        action.trim().trim_matches('"'),
        "Novy kredit" | "Kredit" | "AKTIVACIA" | "BLOKOVANA"
    )
}

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

/// Run the in-place backfill against `pool`, reading legacy data from `mdb_path`.
///
/// Idempotent: only updates rows where `service_id IS NULL`. Sets
/// `legacy_backfilled = 1` alongside `service_id` so a targeted rollback
/// is possible.
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

    for result in data_reader.records() {
        let r = result.context("parse legacy Data row")?;
        // Header: id_data,id_card,user,action,service,suma_SK,Date,EndDate,suma
        let legacy_card_id = r.get(1).unwrap_or("").trim().to_string();
        let action = r.get(3).unwrap_or("").trim();
        let legacy_service = r.get(4).unwrap_or("").trim().trim_matches('"').to_string();
        let date = r.get(6).unwrap_or("").trim().to_string();
        let amount_eur: f64 = r.get(8).unwrap_or("0").trim().parse().unwrap_or(0.0);

        if !legacy_action_has_service(action) {
            continue;
        }
        if legacy_service.is_empty() {
            continue;
        }

        let barcode = match legacy_card_to_barcode.get(&legacy_card_id) {
            Some(bc) => bc,
            None => {
                report.orphan_card += 1;
                continue;
            }
        };

        let new_name = match map_legacy_service_name(&legacy_service) {
            Some(n) => n,
            None => {
                warn!(
                    "unknown legacy service '{legacy_service}' on row card={legacy_card_id} \
                     (will be skipped — extend map_legacy_service_name if this should be backfilled)"
                );
                report.unknown_service += 1;
                continue;
            }
        };
        let svc_id = match service_ids.get(new_name) {
            Some(id) => *id,
            None => {
                warn!(
                    "target DB has no service named '{new_name}' (legacy '{legacy_service}'); \
                     run V8/V9 migrations first"
                );
                report.unknown_service += 1;
                continue;
            }
        };

        // Match (barcode, created_at_string, amount with epsilon) and only
        // touch rows that don't already have a service_id. Prod stores the
        // amount as negative for debits; legacy `suma` is positive.
        let updated_ids: Vec<(i64,)> = sqlx::query_as(
            "UPDATE transactions
                SET service_id = ?, legacy_backfilled = 1
              WHERE id IN (
                SELECT t.id
                  FROM transactions t
                  JOIN cards c ON c.id = t.card_id
                 WHERE c.barcode = ?
                   AND t.created_at = ?
                   AND ABS(t.amount + ?) < 0.005
                   AND t.service_id IS NULL
              )
              RETURNING id",
        )
        .bind(svc_id)
        .bind(barcode)
        .bind(&date)
        .bind(amount_eur)
        .fetch_all(pool)
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
                        AND ABS(t.amount + ?) < 0.005
                      LIMIT 1",
                )
                .bind(barcode)
                .bind(&date)
                .bind(amount_eur)
                .fetch_optional(pool)
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
                    "ambiguous: legacy row card={legacy_card_id} date={date} amount={amount_eur} \
                     matched {n} prod rows: {:?}",
                    updated_ids.iter().map(|(i,)| *i).collect::<Vec<_>>()
                );
            }
        }
    }

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
        "  TOTAL: matched={} already-set={} unmatched={} ambiguous={} orphan_card={} unknown_service={}",
        report.matched,
        report.already_set,
        report.unmatched,
        report.ambiguous,
        report.orphan_card,
        report.unknown_service
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
                   AND ABS(t.amount + ?) < 0.005
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
             VALUES (?, -1.66, 'debit', '11/06/08 21:31:04')",
        )
        .bind(card_id)
        .execute(&pool)
        .await
        .unwrap();
        let svc_id = doplnky_service_id(&pool).await;

        let first = backfill_one(&pool, svc_id, "LEG-1", "11/06/08 21:31:04", 1.66).await;
        assert_eq!(first.len(), 1, "first run should match the row");

        let second = backfill_one(&pool, svc_id, "LEG-1", "11/06/08 21:31:04", 1.66).await;
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
             VALUES (?, ?, -1.66, 'debit', '11/06/08 21:31:04')",
        )
        .bind(card_id)
        .bind(fitness_id)
        .execute(&pool)
        .await
        .unwrap();
        let svc_id = doplnky_service_id(&pool).await;

        let updated = backfill_one(&pool, svc_id, "LEG-2", "11/06/08 21:31:04", 1.66).await;
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
             VALUES (?, -1.66, 'debit', '11/06/08 21:31:04'),
                    (?, -1.66, 'debit', '11/06/08 21:31:04')",
        )
        .bind(card_id)
        .bind(card_id)
        .execute(&pool)
        .await
        .unwrap();
        let svc_id = doplnky_service_id(&pool).await;

        let updated = backfill_one(&pool, svc_id, "LEG-3", "11/06/08 21:31:04", 1.66).await;
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
}
