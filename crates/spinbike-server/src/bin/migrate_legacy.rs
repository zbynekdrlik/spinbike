//! CLI tool to migrate data from the legacy VB6 Access database into the new SQLite schema.
//!
//! Usage (fresh import):
//!   migrate-legacy --mdb-path <path/to/db.mdb> --output <path/to/spinbike.db>
//!
//! Usage (in-place backfill):
//!   migrate-legacy --backfill --mdb-path <path/to/db.mdb> --target <path/to/spinbike.db>
//!
//! Requires `mdb-export` (from mdbtools) to be installed on the system.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use spinbike_server::auth;
use spinbike_server::db;

enum Mode {
    FreshImport { mdb_path: PathBuf, target: PathBuf },
    Backfill { mdb_path: PathBuf, target: PathBuf },
}

fn parse_args() -> Result<Mode> {
    let args: Vec<String> = std::env::args().collect();
    let mut mdb_path: Option<PathBuf> = None;
    let mut target: Option<PathBuf> = None;
    let mut backfill = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--mdb-path" => {
                i += 1;
                mdb_path = Some(PathBuf::from(
                    args.get(i).context("--mdb-path requires a value")?,
                ));
            }
            "--output" | "--target" => {
                i += 1;
                target = Some(PathBuf::from(
                    args.get(i).context("--output/--target requires a value")?,
                ));
            }
            "--backfill" => backfill = true,
            other => bail!("Unknown argument: {other}"),
        }
        i += 1;
    }

    let mdb_path = mdb_path.context("Missing required argument: --mdb-path <path>")?;
    let target = target.context("Missing required argument: --output/--target <path>")?;
    if !mdb_path.exists() {
        bail!("MDB file not found: {}", mdb_path.display());
    }

    Ok(if backfill {
        Mode::Backfill { mdb_path, target }
    } else {
        Mode::FreshImport { mdb_path, target }
    })
}

/// Run `mdb-export` on the given table and return the CSV output as a string.
fn export_table(mdb_path: &PathBuf, table: &str) -> Result<String> {
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

// Legacy service-name mapping is the single source of truth in
// `db::backfill::map_legacy_service_name`. Importing it here keeps the
// fresh-import path and the in-place backfill mode in lockstep — adding
// or removing a legacy service requires ONE edit, not two.
use spinbike_server::db::backfill::map_legacy_service_name;

/// Parse legacy EndDate strings in `MM/DD/YY HH:MM:SS` format
/// (e.g. "12/05/08 00:00:00") to a `NaiveDate`. Blank/unparsable → None.
fn parse_legacy_end_date(s: &str) -> anyhow::Result<Option<chrono::NaiveDate>> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    // Two-digit year — chrono parses "08" as year 8 without `%y` flag.
    match chrono::NaiveDateTime::parse_from_str(trimmed, "%m/%d/%y %H:%M:%S") {
        Ok(dt) => Ok(Some(dt.date())),
        Err(_) => Ok(None),
    }
}

/// New-convention mapping output for a single legacy row. The migrator writes
/// `action` and `amount` directly; downstream consumers (classifier, SQL
/// filters) treat these rows identically to live writes from the new server.
#[derive(Debug, PartialEq)]
struct MappedTxn {
    action: &'static str,
    amount: f64,
}

/// Map a legacy action + amount + valid_until-presence into the new
/// signed-amount + neutral-action convention used everywhere in the rewrite
/// (charge / topup / visit / storno). Returns None for actions that should
/// not produce a transaction row (e.g., BLOKOVANA — handled by setting
/// card.blocked = true at the call site).
///
/// Mirrors the V12 schema migration table — re-imports via this function
/// produce rows that V12 would already consider new-convention, so V12 is a
/// no-op on freshly imported data.
fn map_legacy(action: &str, amount: f64, has_valid_until: bool) -> Option<MappedTxn> {
    let mapped = match action.trim().trim_matches('"') {
        "Debet" | "Vstup" => {
            if amount == 0.0 && !has_valid_until {
                MappedTxn {
                    action: "visit",
                    amount: 0.0,
                }
            } else {
                // A real debit / paid visit / pass purchase — flip sign so
                // amount < 0 (or amount = 0 for free pass purchases when
                // has_valid_until is true).
                MappedTxn {
                    action: "charge",
                    amount: -amount.abs(),
                }
            }
        }
        "Kredit" | "Novy kredit" | "AKTIVACIA" if amount < 0.0 => {
            // V12 mirror: a single 2010 prod row exists with action='credit'
            // and amount=-30.0 (manual correction in the legacy DB). V12 maps
            // it to (charge, amount) — preserve that here so re-imports of
            // any such future row land in the same shape.
            MappedTxn {
                action: "charge",
                amount,
            }
        }
        "Kredit" | "Novy kredit" | "AKTIVACIA" => MappedTxn {
            action: "topup",
            amount: amount.abs(),
        },
        "Storno" if amount > 0.0 => MappedTxn {
            action: "topup",
            amount,
        },
        "Storno" => MappedTxn {
            action: "storno",
            amount,
        },
        "BLOKOVANA" => return None,
        other => {
            warn!(
                "Unknown legacy action: '{other}' (amount={amount}), \
                 mapping to 'topup' with positive amount as fallback"
            );
            MappedTxn {
                action: "topup",
                amount: amount.abs(),
            }
        }
    };
    Some(mapped)
}

async fn run_fresh_import(mdb_path: PathBuf, output_path: PathBuf) -> Result<()> {
    // Remove existing output file to start fresh.
    if output_path.exists() {
        std::fs::remove_file(&output_path).with_context(|| {
            format!(
                "Failed to remove existing output: {}",
                output_path.display()
            )
        })?;
    }

    info!("Opening output database: {}", output_path.display());
    let pool = db::create_pool(&output_path).await?;
    db::run_migrations(&pool).await?;

    // --- Import instructors ---
    info!("Importing instructors from t_Inst...");
    let inst_csv = export_table(&mdb_path, "t_Inst")?;
    let mut inst_reader = csv::Reader::from_reader(Cursor::new(&inst_csv));
    let mut instructor_count = 0u32;

    for result in inst_reader.records() {
        let record = result.context("Failed to parse instructor CSV record")?;
        let name = record.get(1).context("Missing Instruktor column")?.trim();
        if name.is_empty() {
            continue;
        }

        sqlx::query("INSERT INTO instructors (name) VALUES (?)")
            .bind(name)
            .execute(&pool)
            .await
            .with_context(|| format!("Failed to insert instructor '{name}'"))?;

        instructor_count += 1;
    }
    info!("Imported {instructor_count} instructors");

    // --- Import cards ---
    info!("Importing cards from card table...");
    let card_csv = export_table(&mdb_path, "card")?;
    let mut card_reader = csv::Reader::from_reader(Cursor::new(&card_csv));

    // Map legacy card Id -> new card id for transaction mapping.
    let mut legacy_card_map: HashMap<String, i64> = HashMap::new();
    let mut card_count = 0u32;

    for result in card_reader.records() {
        let record = result.context("Failed to parse card CSV record")?;
        // Header: Id,BarCode,TypCard,Blocked,Credit_SK,Name,LastName,Firma,phone,Debet,FirstCredit,EndDate,TimeCard,Credit
        let legacy_id = record.get(0).context("Missing Id column")?.trim();
        let barcode = record.get(1).context("Missing BarCode column")?.trim();
        let blocked: i64 = record
            .get(3)
            .context("Missing Blocked column")?
            .trim()
            .parse()
            .unwrap_or(0);
        let first_name = record.get(5).unwrap_or("").trim();
        let last_name = record.get(6).unwrap_or("").trim();
        let company = record.get(7).unwrap_or("").trim();
        let phone = record.get(8).unwrap_or("").trim();
        let credit_eur: f64 = record
            .get(13)
            .context("Missing Credit (EUR) column")?
            .trim()
            .parse()
            .unwrap_or(0.0);

        if barcode.is_empty() {
            warn!("Skipping card with empty barcode (legacy id={legacy_id})");
            continue;
        }

        let first_name_opt = if first_name.is_empty() {
            None
        } else {
            Some(first_name)
        };
        let last_name_opt = if last_name.is_empty() {
            None
        } else {
            Some(last_name)
        };
        let company_opt = if company.is_empty() {
            None
        } else {
            Some(company)
        };
        let phone_opt = if phone.is_empty() { None } else { Some(phone) };

        // allow_debit = 1 for all cards (legacy app behavior).
        let insert_result: Result<i64, _> = sqlx::query_scalar(
            "INSERT INTO cards (barcode, blocked, credit, allow_debit, first_name, last_name, company, phone)
             VALUES (?, ?, ?, 1, ?, ?, ?, ?) RETURNING id",
        )
        .bind(barcode)
        .bind(blocked)
        .bind(credit_eur)
        .bind(first_name_opt)
        .bind(last_name_opt)
        .bind(company_opt)
        .bind(phone_opt)
        .fetch_one(&pool)
        .await;

        let new_id = match insert_result {
            Ok(id) => id,
            Err(e) if e.to_string().contains("UNIQUE constraint") => {
                warn!("Skipping duplicate barcode {barcode} (legacy id={legacy_id})");
                // Map to existing card so transactions still link correctly
                let existing: i64 = sqlx::query_scalar("SELECT id FROM cards WHERE barcode = ?")
                    .bind(barcode)
                    .fetch_one(&pool)
                    .await
                    .with_context(|| format!("Failed to look up existing barcode={barcode}"))?;
                legacy_card_map.insert(legacy_id.to_string(), existing);
                continue;
            }
            Err(e) => {
                return Err(e).with_context(|| format!("Failed to insert card barcode={barcode}"));
            }
        };

        legacy_card_map.insert(legacy_id.to_string(), new_id);
        card_count += 1;
    }
    info!("Imported {card_count} cards");

    // --- Import transactions ---
    info!("Importing transactions from Data table...");
    let data_csv = export_table(&mdb_path, "Data")?;
    let mut data_reader = csv::Reader::from_reader(Cursor::new(&data_csv));

    // Load service name → id map once for legacy service name resolution.
    let service_ids: std::collections::HashMap<String, i64> =
        sqlx::query_as::<_, (String, i64)>("SELECT name_sk, id FROM services")
            .fetch_all(&pool)
            .await
            .context("Failed to load services for legacy mapping")?
            .into_iter()
            .collect();

    // Track cards that should be blocked (from BLOKOVANA actions).
    let mut blocked_cards: Vec<i64> = Vec::new();
    let mut txn_count = 0u32;
    let mut skipped_count = 0u32;

    for result in data_reader.records() {
        let record = result.context("Failed to parse Data CSV record")?;
        // Header: id_data,id_card,user,action,service,suma_SK,Date,EndDate,suma
        let legacy_card_id = record.get(1).context("Missing id_card column")?.trim();
        let action = record.get(3).context("Missing action column")?.trim();
        let amount_eur: f64 = record
            .get(8)
            .context("Missing suma (EUR) column")?
            .trim()
            .parse()
            .unwrap_or(0.0);
        let date = record.get(6).context("Missing Date column")?.trim();

        let legacy_service = record.get(4).context("Missing service column")?.trim();
        let end_date_raw = record.get(7).context("Missing EndDate column")?.trim();

        let service_id: Option<i64> = map_legacy_service_name(legacy_service)
            .and_then(|new_name| service_ids.get(new_name).copied());
        let valid_until = parse_legacy_end_date(end_date_raw)?;

        let new_card_id = legacy_card_map.get(legacy_card_id).copied();

        match map_legacy(action, amount_eur, valid_until.is_some()) {
            None => {
                // BLOKOVANA — mark card as blocked.
                if let Some(card_id) = new_card_id {
                    blocked_cards.push(card_id);
                }
                skipped_count += 1;
            }
            Some(mapped) => {
                // Format the legacy date for created_at.
                // Legacy format: "MM/DD/YY HH:MM:SS" — store as-is since SQLite is flexible.
                sqlx::query(
                    "INSERT INTO transactions (card_id, amount, action, created_at, service_id, valid_until)
                     VALUES (?, ?, ?, ?, ?, ?)",
                )
                .bind(new_card_id)
                .bind(mapped.amount)
                .bind(mapped.action)
                .bind(date)
                .bind(service_id)
                .bind(valid_until)
                .execute(&pool)
                .await
                .with_context(|| {
                    format!(
                        "Failed to insert transaction: card={legacy_card_id}, action={action}"
                    )
                })?;

                txn_count += 1;
            }
        }
    }

    // Apply blocked status from BLOKOVANA actions.
    for card_id in &blocked_cards {
        sqlx::query("UPDATE cards SET blocked = 1 WHERE id = ?")
            .bind(card_id)
            .execute(&pool)
            .await?;
    }

    info!(
        "Imported {txn_count} transactions, skipped {skipped_count} BLOKOVANA actions (applied as card blocks)"
    );

    // --- Create admin account ---
    info!("Creating initial admin account...");
    let admin_email = "admin@spinbike.local";

    let existing: Option<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE email = ?")
        .bind(admin_email)
        .fetch_optional(&pool)
        .await?;

    if existing.is_some() {
        info!("Admin account already exists, skipping");
    } else {
        let password_hash =
            auth::hash_password("changeme").context("Failed to hash admin password")?;

        sqlx::query("INSERT INTO users (email, password_hash, name, role) VALUES (?, ?, ?, ?)")
            .bind(admin_email)
            .bind(&password_hash)
            .bind("Admin")
            .bind("admin")
            .execute(&pool)
            .await
            .context("Failed to create admin user")?;

        info!("Created admin account: {admin_email} / changeme");
    }

    // --- Verify seed services ---
    let svc_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM services")
        .fetch_one(&pool)
        .await?;
    info!("Services in database: {svc_count}");
    if svc_count == 0 {
        warn!("No services found — migrations should have seeded them. Check migration V1.");
    }

    // --- Summary ---
    info!("=== Migration complete ===");
    info!("  Instructors: {instructor_count}");
    info!("  Cards:       {card_count}");
    info!("  Transactions: {txn_count}");
    info!("  Admin user:  {admin_email}");
    info!("  Services:    {svc_count}");
    info!("  Output:      {}", output_path.display());

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("migrate_legacy=info".parse()?),
        )
        .init();

    match parse_args()? {
        Mode::FreshImport { mdb_path, target } => run_fresh_import(mdb_path, target).await,
        Mode::Backfill { mdb_path, target } => {
            if !target.exists() {
                bail!(
                    "--backfill requires an existing target DB: {}",
                    target.display()
                );
            }
            let pool = db::create_pool(&target).await?;
            db::run_migrations(&pool).await?;
            let report = db::backfill::run(&pool, &mdb_path).await?;
            info!(
                "Backfill done: matched={} already_set={} unmatched={} ambiguous={} orphan_card={} unknown_service={} malformed_date={}",
                report.matched,
                report.already_set,
                report.unmatched,
                report.ambiguous,
                report.orphan_card,
                report.unknown_service,
                report.malformed_date
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_legacy_debet_positive_amount_becomes_negative_charge() {
        assert_eq!(
            map_legacy("Debet", 3.0, false),
            Some(MappedTxn {
                action: "charge",
                amount: -3.0
            })
        );
    }

    #[test]
    fn map_legacy_debet_zero_no_valid_until_becomes_visit() {
        assert_eq!(
            map_legacy("Debet", 0.0, false),
            Some(MappedTxn {
                action: "visit",
                amount: 0.0
            })
        );
    }

    #[test]
    fn map_legacy_debet_zero_with_valid_until_becomes_zero_charge() {
        assert_eq!(
            map_legacy("Debet", 0.0, true),
            Some(MappedTxn {
                action: "charge",
                amount: 0.0
            })
        );
    }

    #[test]
    fn map_legacy_debet_with_valid_until_becomes_negative_charge() {
        assert_eq!(
            map_legacy("Debet", 28.0, true),
            Some(MappedTxn {
                action: "charge",
                amount: -28.0
            })
        );
    }

    #[test]
    fn map_legacy_vstup_positive_amount_becomes_negative_charge() {
        assert_eq!(
            map_legacy("Vstup", 2.5, false),
            Some(MappedTxn {
                action: "charge",
                amount: -2.5
            })
        );
    }

    #[test]
    fn map_legacy_vstup_zero_no_valid_until_becomes_visit() {
        assert_eq!(
            map_legacy("Vstup", 0.0, false),
            Some(MappedTxn {
                action: "visit",
                amount: 0.0
            })
        );
    }

    #[test]
    fn map_legacy_kredit_becomes_topup() {
        assert_eq!(
            map_legacy("Kredit", 30.0, false),
            Some(MappedTxn {
                action: "topup",
                amount: 30.0
            })
        );
    }

    #[test]
    fn map_legacy_novy_kredit_becomes_topup() {
        assert_eq!(
            map_legacy("Novy kredit", 30.0, false),
            Some(MappedTxn {
                action: "topup",
                amount: 30.0
            })
        );
    }

    #[test]
    fn map_legacy_aktivacia_becomes_topup() {
        assert_eq!(
            map_legacy("AKTIVACIA", 30.0, false),
            Some(MappedTxn {
                action: "topup",
                amount: 30.0
            })
        );
    }

    #[test]
    fn map_legacy_kredit_negative_amount_mirrors_v12_to_charge() {
        // V12 maps action='credit' AND amount < 0 to (charge, amount).
        // The importer must mirror that, otherwise a re-import of the
        // single documented 2010 prod row would diverge from V12's
        // post-state.
        assert_eq!(
            map_legacy("Kredit", -30.0, false),
            Some(MappedTxn {
                action: "charge",
                amount: -30.0
            })
        );
    }

    #[test]
    fn map_legacy_storno_positive_becomes_topup() {
        assert_eq!(
            map_legacy("Storno", 2.5, false),
            Some(MappedTxn {
                action: "topup",
                amount: 2.5
            })
        );
    }

    #[test]
    fn map_legacy_storno_zero_stays_storno() {
        assert_eq!(
            map_legacy("Storno", 0.0, false),
            Some(MappedTxn {
                action: "storno",
                amount: 0.0
            })
        );
    }

    #[test]
    fn map_legacy_blokovana_returns_none() {
        assert_eq!(map_legacy("BLOKOVANA", 0.0, false), None);
    }

    #[test]
    fn map_legacy_unknown_falls_back_to_positive_topup() {
        assert_eq!(
            map_legacy("MysteryAction", 5.0, false),
            Some(MappedTxn {
                action: "topup",
                amount: 5.0
            })
        );
    }

    #[test]
    fn map_legacy_unknown_negative_amount_falls_back_to_positive_topup() {
        // .abs() in the unknown fallback ensures even a malformed
        // negative-amount mystery action lands as a positive-amount
        // topup. Kills cargo-mutants .abs() removal mutants.
        assert_eq!(
            map_legacy("MysteryAction", -5.0, false),
            Some(MappedTxn {
                action: "topup",
                amount: 5.0
            })
        );
    }

    #[test]
    fn map_legacy_strips_quotes_and_whitespace() {
        assert_eq!(
            map_legacy("  \"Debet\"  ", 3.0, false),
            Some(MappedTxn {
                action: "charge",
                amount: -3.0
            })
        );
    }

    #[test]
    fn parse_end_date_valid() {
        assert_eq!(
            parse_legacy_end_date("12/05/08 00:00:00").unwrap(),
            Some(chrono::NaiveDate::from_ymd_opt(2008, 12, 5).unwrap())
        );
    }

    #[test]
    fn parse_end_date_empty_is_none() {
        assert_eq!(parse_legacy_end_date("").unwrap(), None);
        assert_eq!(parse_legacy_end_date("   ").unwrap(), None);
    }

    #[test]
    fn parse_end_date_garbage_is_none() {
        assert_eq!(parse_legacy_end_date("not a date").unwrap(), None);
    }

    #[test]
    fn map_legacy_service_known_names() {
        assert_eq!(
            map_legacy_service_name("Casova karta"),
            Some("Mesačný preplatok")
        );
        assert_eq!(
            map_legacy_service_name("Fitnes"),
            Some(spinbike_core::services::FITNESS_NAME_EN)
        );
        assert_eq!(
            map_legacy_service_name("Spinbike"),
            Some(spinbike_core::services::SPINNING_NAME_EN)
        );
    }

    #[test]
    fn map_legacy_service_extended_names() {
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
    }

    #[test]
    fn map_legacy_service_unknown_returns_none() {
        assert_eq!(map_legacy_service_name("Something else"), None);
        assert_eq!(map_legacy_service_name(""), None);
        // Storno is intentionally unmapped — the action='storno' label suffices.
        assert_eq!(map_legacy_service_name("Storno"), None);
        // Iont had zero historical sales — YAGNI.
        assert_eq!(map_legacy_service_name("Iont"), None);
    }

    #[tokio::test]
    async fn importer_preserves_service_and_end_date() {
        use spinbike_server::db::{create_memory_pool, run_migrations};
        let pool = create_memory_pool().await.unwrap();
        run_migrations(&pool).await.unwrap();

        // Seed a card row so id 1 exists in the new DB.
        sqlx::query(
            "INSERT INTO cards (id, barcode, allow_debit, search_text) VALUES (1, 'C1', 1, 'c1')",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Mimic what the import loop does for one row.
        let service_ids: std::collections::HashMap<String, i64> =
            sqlx::query_as::<_, (String, i64)>("SELECT name_sk, id FROM services")
                .fetch_all(&pool)
                .await
                .unwrap()
                .into_iter()
                .collect();

        let service_id = map_legacy_service_name("Casova karta")
            .and_then(|n| service_ids.get(n).copied())
            .unwrap();
        let valid_until = parse_legacy_end_date("12/05/08 00:00:00").unwrap();

        sqlx::query(
            "INSERT INTO transactions (card_id, amount, action, created_at, service_id, valid_until)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(1_i64).bind(-19.92).bind("charge").bind("11/06/08 21:20:24")
        .bind(Some(service_id)).bind(valid_until)
        .execute(&pool).await.unwrap();

        let row: (Option<String>, Option<chrono::NaiveDate>) = sqlx::query_as(
            "SELECT s.name_sk, t.valid_until FROM transactions t
             LEFT JOIN services s ON s.id = t.service_id WHERE t.card_id = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0.as_deref(), Some("Mesačný preplatok"));
        assert_eq!(
            row.1,
            Some(chrono::NaiveDate::from_ymd_opt(2008, 12, 5).unwrap())
        );
    }
}
