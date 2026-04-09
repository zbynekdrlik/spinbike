//! CLI tool to migrate data from the legacy VB6 Access database into the new SQLite schema.
//!
//! Usage:
//!   migrate-legacy --mdb-path <path/to/db.mdb> --output <path/to/spinbike.db>
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

fn parse_args() -> Result<(PathBuf, PathBuf)> {
    let args: Vec<String> = std::env::args().collect();
    let mut mdb_path: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--mdb-path" => {
                i += 1;
                mdb_path = Some(PathBuf::from(
                    args.get(i).context("--mdb-path requires a value")?,
                ));
            }
            "--output" => {
                i += 1;
                output = Some(PathBuf::from(
                    args.get(i).context("--output requires a value")?,
                ));
            }
            other => bail!("Unknown argument: {other}"),
        }
        i += 1;
    }

    let mdb_path = mdb_path.context("Missing required argument: --mdb-path <path>")?;
    let output = output.context("Missing required argument: --output <path>")?;

    if !mdb_path.exists() {
        bail!("MDB file not found: {}", mdb_path.display());
    }

    Ok((mdb_path, output))
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

    Ok(String::from_utf8(output.stdout)
        .with_context(|| format!("mdb-export output for '{table}' is not valid UTF-8"))?)
}

/// Map a legacy action string to the new action format.
/// Returns None for actions that should not create a transaction (e.g., BLOKOVANA).
fn map_action(action: &str) -> Option<&'static str> {
    match action.trim().trim_matches('"') {
        "Debet" => Some("debit"),
        "Kredit" | "Novy kredit" => Some("credit"),
        "AKTIVACIA" => Some("activation"),
        "Storno" => Some("storno"),
        "Vstup" => Some("debit"),
        "BLOKOVANA" => None, // handled by setting card.blocked = true
        other => {
            warn!("Unknown legacy action: '{other}', mapping to 'unknown'");
            Some("unknown")
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("migrate_legacy=info".parse()?),
        )
        .init();

    let (mdb_path, output_path) = parse_args()?;

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
        let credit_eur: f64 = record
            .get(13)
            .context("Missing Credit (EUR) column")?
            .trim()
            .parse()
            .unwrap_or(0.0);
        let debet: i64 = record
            .get(9)
            .context("Missing Debet column")?
            .trim()
            .parse()
            .unwrap_or(0);

        if barcode.is_empty() {
            warn!("Skipping card with empty barcode (legacy id={legacy_id})");
            continue;
        }

        let new_id: i64 = sqlx::query_scalar(
            "INSERT INTO cards (barcode, blocked, credit, allow_debit) VALUES (?, ?, ?, ?) RETURNING id",
        )
        .bind(barcode)
        .bind(blocked)
        .bind(credit_eur)
        .bind(debet)
        .fetch_one(&pool)
        .await
        .with_context(|| format!("Failed to insert card barcode={barcode}"))?;

        legacy_card_map.insert(legacy_id.to_string(), new_id);
        card_count += 1;
    }
    info!("Imported {card_count} cards");

    // --- Import transactions ---
    info!("Importing transactions from Data table...");
    let data_csv = export_table(&mdb_path, "Data")?;
    let mut data_reader = csv::Reader::from_reader(Cursor::new(&data_csv));

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

        let new_card_id = legacy_card_map.get(legacy_card_id).copied();

        match map_action(action) {
            None => {
                // BLOKOVANA — mark card as blocked.
                if let Some(card_id) = new_card_id {
                    blocked_cards.push(card_id);
                }
                skipped_count += 1;
            }
            Some(mapped_action) => {
                // Format the legacy date for created_at.
                // Legacy format: "MM/DD/YY HH:MM:SS" — store as-is since SQLite is flexible.
                sqlx::query(
                    "INSERT INTO transactions (card_id, amount, action, created_at) VALUES (?, ?, ?, ?)",
                )
                .bind(new_card_id)
                .bind(amount_eur)
                .bind(mapped_action)
                .bind(date)
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
