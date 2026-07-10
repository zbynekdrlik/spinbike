use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, patch},
};
use serde::Deserialize;

use crate::AppState;
use crate::auth::AuthUser;
use crate::db::transactions::NOTE_MAX_CHARS;
use crate::error::ApiError;
use crate::routes::internal_error;
use spinbike_core::errors::ErrorCode;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/transactions/{id}", delete(void_transaction))
        .route(
            "/api/transactions/{id}/valid-until",
            patch(patch_valid_until),
        )
        .route("/api/transactions/{id}/note", patch(patch_note))
        .route("/api/transactions/{id}/created-at", patch(patch_created_at))
}

#[derive(sqlx::FromRow)]
struct TxMini {
    amount: f64,
    user_id: Option<i64>,
    deleted_at: Option<String>,
    valid_until: Option<String>,
    created_at: String,
}

#[derive(Deserialize)]
struct PatchValidUntilReq {
    valid_until: chrono::NaiveDate,
}

#[derive(serde::Serialize)]
struct PatchValidUntilResp {
    id: i64,
    valid_until: chrono::NaiveDate,
}

#[derive(Deserialize)]
struct PatchNoteReq {
    /// New note. `None` (or absent) → clear the column.
    #[serde(default)]
    note: Option<String>,
}

#[derive(serde::Serialize)]
struct PatchNoteResp {
    id: i64,
    note: Option<String>,
}

#[derive(Deserialize)]
struct PatchCreatedAtReq {
    created_at_date: chrono::NaiveDate,
}

#[derive(serde::Serialize)]
struct PatchCreatedAtResp {
    id: i64,
    created_at_date: chrono::NaiveDate,
}

async fn void_transaction(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    if !claims.role.can_process_payments() {
        return Err(ApiError::Forbidden(ErrorCode::StaffRequired));
    }

    let mut tx = state.pool.begin().await.map_err(internal_error)?;

    let row: Option<TxMini> = sqlx::query_as(
        "SELECT amount, user_id, deleted_at, valid_until, created_at FROM transactions WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal_error)?;

    let Some(row) = row else {
        return Err(ApiError::NotFound(ErrorCode::TransactionNotFound));
    };
    if row.deleted_at.is_some() {
        return Err(ApiError::NotFound(ErrorCode::TransactionAlreadyVoided));
    }

    sqlx::query("UPDATE transactions SET deleted_at = datetime('now') WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal_error)?;

    if let Some(user_id) = row.user_id {
        // Single-formula credit reversal works because amounts are SIGNED
        // in the transactions table:
        //   - charges/visits store NEGATIVE amounts → `credit - (-X)` = `credit + X` (refund)
        //   - top-ups       store POSITIVE amounts → `credit - (+X)` = `credit - X` (claw-back)
        // ROUND keeps SQLite from drifting on float math.
        sqlx::query("UPDATE users SET credit = ROUND(credit - ?, 2) WHERE id = ?")
            .bind(row.amount)
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .map_err(internal_error)?;
    }

    tx.commit().await.map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn patch_valid_until(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<PatchValidUntilReq>,
) -> Result<Json<PatchValidUntilResp>, ApiError> {
    if !claims.role.can_process_payments() {
        return Err(ApiError::Forbidden(ErrorCode::StaffRequired));
    }

    let row: Option<TxMini> = sqlx::query_as(
        "SELECT amount, user_id, valid_until, deleted_at, created_at FROM transactions WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?;

    let Some(row) = row else {
        return Err(ApiError::NotFound(ErrorCode::TransactionNotFound));
    };
    if row.valid_until.is_none() {
        return Err(super::bad_request(
            "Only pass transactions have valid_until",
        ));
    }

    sqlx::query("UPDATE transactions SET valid_until = ? WHERE id = ?")
        .bind(body.valid_until.to_string())
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(internal_error)?;

    Ok(Json(PatchValidUntilResp {
        id,
        valid_until: body.valid_until,
    }))
}

async fn patch_note(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<PatchNoteReq>,
) -> Result<Json<PatchNoteResp>, ApiError> {
    // Same role gate as void / valid-until edit — staff only.
    if !claims.role.can_manage_cards() {
        return Err(ApiError::Forbidden(ErrorCode::StaffRequired));
    }

    // 200-char cap, counted in characters (not bytes) so Slovak diacritics
    // don't count double. Empty/whitespace becomes NULL.
    let normalized: Option<String> = match body.note.as_deref() {
        Some(s) if !s.trim().is_empty() => {
            if s.chars().count() > NOTE_MAX_CHARS {
                return Err(super::bad_request("Note must be 200 characters or fewer"));
            }
            Some(s.to_string())
        }
        _ => None,
    };

    let row: Option<TxMini> = sqlx::query_as(
        "SELECT amount, user_id, deleted_at, valid_until, created_at FROM transactions WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?;

    let Some(row) = row else {
        return Err(ApiError::NotFound(ErrorCode::TransactionNotFound));
    };
    if row.deleted_at.is_some() {
        return Err(ApiError::conflict(ErrorCode::NoteOnVoidedTransaction));
    }

    sqlx::query("UPDATE transactions SET note = ? WHERE id = ?")
        .bind(normalized.as_deref())
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(internal_error)?;

    Ok(Json(PatchNoteResp {
        id,
        note: normalized,
    }))
}

async fn patch_created_at(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<PatchCreatedAtReq>,
) -> Result<Json<PatchCreatedAtResp>, ApiError> {
    if !claims.role.can_manage_cards() {
        return Err(ApiError::Forbidden(ErrorCode::StaffRequired));
    }

    let row: Option<TxMini> = sqlx::query_as(
        "SELECT amount, user_id, deleted_at, valid_until, created_at FROM transactions WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?;

    let Some(row) = row else {
        return Err(ApiError::NotFound(ErrorCode::TransactionNotFound));
    };
    if row.deleted_at.is_some() {
        return Err(ApiError::conflict(ErrorCode::DateOnVoidedTransaction));
    }

    // 30-day window check (inclusive). Future dates are also rejected — same
    // single error message covers both branches per spec.
    let today = chrono::Local::now().date_naive();
    let earliest = today - chrono::Duration::days(30);
    if body.created_at_date < earliest || body.created_at_date > today {
        return Err(super::bad_request("Date must be within last 30 days"));
    }

    // The stored created_at is UTC text (SQLite's datetime('now') is UTC).
    // The user picks dates in Bratislava local time. To preserve the user's
    // intent, convert UTC → Bratislava, swap the local date, then convert
    // back to UTC for storage.
    use chrono::TimeZone;
    let bratislava = chrono_tz::Europe::Bratislava;
    let existing_utc =
        chrono::NaiveDateTime::parse_from_str(row.created_at.trim(), "%Y-%m-%d %H:%M:%S")
            .ok()
            .or_else(|| {
                chrono::NaiveDateTime::parse_from_str(row.created_at.trim(), "%Y-%m-%dT%H:%M:%S")
                    .ok()
            });
    let new_utc = match existing_utc {
        Some(utc_dt) => {
            let local_dt = bratislava.from_utc_datetime(&utc_dt);
            let local_time = local_dt.time();
            let new_local_naive = chrono::NaiveDateTime::new(body.created_at_date, local_time);
            // Pick .earliest() on DST-ambiguous local datetimes; treat
            // gap (LocalResult::None) the same way via .single() fallback.
            bratislava
                .from_local_datetime(&new_local_naive)
                .earliest()
                .map(|dt| dt.naive_utc())
                .unwrap_or_else(|| new_local_naive)
        }
        None => {
            // Existing value didn't parse — fall back to noon UTC on the
            // chosen local date. This branch shouldn't fire on real data.
            chrono::NaiveDateTime::new(
                body.created_at_date,
                chrono::NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
            )
        }
    };
    let new_value = new_utc.format("%Y-%m-%d %H:%M:%S").to_string();

    sqlx::query("UPDATE transactions SET created_at = ? WHERE id = ?")
        .bind(&new_value)
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(internal_error)?;

    Ok(Json(PatchCreatedAtResp {
        id,
        created_at_date: body.created_at_date,
    }))
}
