use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post, put},
};
use chrono::Datelike;
use serde::{Deserialize, Serialize};

use spinbike_core::services::CLASS_VISIT_NAMES_EN;
use spinbike_core::stats::{MonthlyBucket, PeriodAgg, PeriodTotals, StatsResponse};

use crate::AppState;
use crate::auth::AuthUser;
use crate::db::transactions::NOTE_MAX_CHARS;
use crate::db::{cards as db, transactions};
use crate::routes::internal_error;

#[derive(Deserialize)]
pub struct LinkCardRequest {
    pub barcode: String,
}

#[derive(Deserialize)]
pub struct ActivateCardRequest {
    pub barcode: String,
    pub initial_credit: f64,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub company: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateCardRequest {
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub company: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
}

#[derive(Deserialize)]
pub struct TopupRequest {
    pub card_id: i64,
    pub amount: f64,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Deserialize)]
pub struct BlockRequest {
    pub card_id: i64,
    pub blocked: bool,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default = "default_search_limit")]
    pub limit: i64,
}

fn default_search_limit() -> i64 {
    10
}

#[derive(Deserialize)]
pub struct TransactionsQuery {
    pub limit: Option<usize>,
    pub before: Option<String>,
}

#[derive(Serialize)]
pub struct CardPass {
    pub valid_until: chrono::NaiveDate,
    pub days_remaining: i32,
    pub transaction_id: i64,
}

#[derive(Serialize)]
pub struct CardResponse {
    pub id: i64,
    pub barcode: String,
    pub user_id: Option<i64>,
    pub blocked: bool,
    pub credit: f64,
    pub allow_debit: bool,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub company: Option<String>,
    pub phone: Option<String>,
    pub pass: Option<CardPass>,
    /// MAX(transactions.created_at) for non-soft-deleted Spinning/Fitness rows.
    /// `None` if the card has never had a qualifying class visit.
    pub last_visit_at: Option<String>,
}

#[derive(Serialize)]
pub struct BalanceResponse {
    pub cards: Vec<CardResponse>,
    pub transactions: Vec<TransactionResponse>,
}

#[derive(Serialize)]
pub struct TransactionResponse {
    pub id: i64,
    pub card_id: Option<i64>,
    pub amount: f64,
    pub action: String,
    pub created_at: String,
    // Slovak label for the service (NULL when the transaction has no service).
    pub service_name_sk: Option<String>,
    // English label for the service (NULL when the transaction has no service).
    pub service_name_en: Option<String>,
    // Stable kind: "generic" | "monthly_pass" | NULL.
    pub service_kind: Option<String>,
    pub valid_until: Option<chrono::NaiveDate>,
    pub deleted_at: Option<String>,
    /// Free-text staff note (≤200 chars). NULL when no note was recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

// Replaces `impl From<&db::CardRow> for CardResponse`.
// Used for single-card handlers (lookup, activate, topup, block, update, link)
// where we need a fresh DB query for the pass.
async fn card_response_from_row(
    pool: &sqlx::SqlitePool,
    c: &db::CardRow,
) -> anyhow::Result<CardResponse> {
    let pass = db::get_card_pass_tx(pool, c.id).await?;
    Ok(card_response_from_row_with_pass(c, pass, None))
}

/// Build a CardResponse from a pre-fetched pass (tx id + date) and last visit timestamp —
/// avoids per-card DB round-trip. Used by list_cards and search_cards which retrieve
/// pass info and last_visit_at in a single query.
fn card_response_from_row_with_pass(
    c: &db::CardRow,
    pass: Option<(i64, chrono::NaiveDate)>,
    last_visit_at: Option<String>,
) -> CardResponse {
    let today = chrono::Local::now().date_naive();
    let pass = pass.map(|(tx_id, d)| CardPass {
        valid_until: d,
        days_remaining: (d - today).num_days() as i32,
        transaction_id: tx_id,
    });
    CardResponse {
        id: c.id,
        barcode: c.barcode.clone(),
        user_id: c.user_id,
        blocked: c.blocked != 0,
        credit: c.credit,
        allow_debit: c.allow_debit != 0,
        first_name: c.first_name.clone(),
        last_name: c.last_name.clone(),
        company: c.company.clone(),
        phone: c.phone.clone(),
        pass,
        last_visit_at,
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/cards", get(list_cards))
        .route("/api/cards/search", get(search_cards))
        .route("/api/cards/link", post(link_card))
        .route("/api/cards/lookup/{barcode}", get(lookup_card))
        .route("/api/cards/activate", post(activate_card))
        .route("/api/cards/topup", post(topup_card))
        .route("/api/cards/block", post(block_card))
        .route("/api/cards/{id}", put(update_card))
        .route("/api/cards/{id}/transactions", get(card_transactions))
        .route("/api/cards/{id}/stats", get(card_stats))
        .route("/api/my/balance", get(my_balance))
}

async fn search_cards(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Query(params): Query<SearchQuery>,
) -> Result<Json<Vec<CardResponse>>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_cards() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff only"})),
        ));
    }
    // Clamp limit to a sane range so clients can't request the whole table.
    let limit = params.limit.clamp(1, 50);
    // Use single JOIN query to get cards + pass info without N+1.
    let rows = db::search_cards_with_pass(&state.pool, &params.q, limit)
        .await
        .map_err(internal_error)?;
    let out = rows
        .iter()
        .map(|(c, pass, last_visit)| card_response_from_row_with_pass(c, *pass, last_visit.clone()))
        .collect();
    Ok(Json(out))
}

async fn list_cards(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<Vec<CardResponse>>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_cards() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff only"})),
        ));
    }
    // Use single JOIN query to get cards + pass info without N+1.
    let rows = db::list_all_cards_with_pass(&state.pool)
        .await
        .map_err(internal_error)?;
    let out = rows
        .iter()
        .map(|(c, pass, last_visit)| card_response_from_row_with_pass(c, *pass, last_visit.clone()))
        .collect();
    Ok(Json(out))
}

async fn link_card(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<LinkCardRequest>,
) -> Result<Json<CardResponse>, (StatusCode, Json<serde_json::Value>)> {
    let card = db::get_card_by_barcode(&state.pool, &body.barcode)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Card not found"})),
            )
        })?;

    if card.user_id.is_some() {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "Card is already linked to a user"})),
        ));
    }

    db::link_card_to_user(&state.pool, card.id, claims.sub)
        .await
        .map_err(internal_error)?;

    let updated = db::get_card_by_barcode(&state.pool, &body.barcode)
        .await
        .map_err(internal_error)?
        .unwrap();

    Ok(Json(
        card_response_from_row(&state.pool, &updated)
            .await
            .map_err(internal_error)?,
    ))
}

async fn lookup_card(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(barcode): Path<String>,
) -> Result<Json<CardResponse>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_cards() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    let card = db::get_card_by_barcode(&state.pool, &barcode)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Card not found"})),
            )
        })?;

    Ok(Json(
        card_response_from_row(&state.pool, &card)
            .await
            .map_err(internal_error)?,
    ))
}

async fn activate_card(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<ActivateCardRequest>,
) -> Result<(StatusCode, Json<CardResponse>), (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_cards() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    // M4: create_card will fail on duplicate barcode due to UNIQUE constraint.
    // The error from context("Failed to create card") is generic enough.
    let card_id = db::create_card_with_info(
        &state.pool,
        &body.barcode,
        0.0,
        body.first_name.as_deref(),
        body.last_name.as_deref(),
        body.company.as_deref(),
        body.phone.as_deref(),
    )
    .await
    .map_err(|e| {
        // `e.to_string()` only shows the outermost anyhow context
        // ("Failed to create card with info"), so we use `{:#}` to include
        // the chain, which carries the actual SQLite UNIQUE message.
        let chain = format!("{e:#}");
        if chain.contains("UNIQUE") || chain.contains("unique") {
            (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": "A card with this barcode already exists"})),
            )
        } else {
            internal_error(e)
        }
    })?;

    if body.initial_credit > 0.0 {
        db::update_credit(&state.pool, card_id, body.initial_credit)
            .await
            .map_err(internal_error)?;

        transactions::create_transaction(
            &state.pool,
            None,
            Some(card_id),
            Some(claims.sub),
            None,
            body.initial_credit,
            "topup",
            None,
        )
        .await
        .map_err(internal_error)?;
    }

    let card = db::get_card_by_barcode(&state.pool, &body.barcode)
        .await
        .map_err(internal_error)?
        .unwrap();

    Ok((
        StatusCode::CREATED,
        Json(
            card_response_from_row(&state.pool, &card)
                .await
                .map_err(internal_error)?,
        ),
    ))
}

async fn topup_card(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<TopupRequest>,
) -> Result<Json<CardResponse>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_cards() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    // I7: Validate topup amount is positive.
    if body.amount <= 0.0 {
        return Err(super::bad_request("Amount must be greater than zero"));
    }
    if let Some(n) = body.note.as_deref()
        && n.chars().count() > NOTE_MAX_CHARS
    {
        return Err(super::bad_request("Note must be 200 characters or fewer"));
    }
    let note_for_db = body.note.as_deref().filter(|s| !s.trim().is_empty());

    db::update_credit(&state.pool, body.card_id, body.amount)
        .await
        .map_err(internal_error)?;

    transactions::create_transaction(
        &state.pool,
        None,
        Some(body.card_id),
        Some(claims.sub),
        None,
        body.amount,
        "topup",
        note_for_db,
    )
    .await
    .map_err(internal_error)?;

    // Re-fetch the card to return updated state.
    let card = sqlx::query_as::<_, db::CardRow>("SELECT * FROM cards WHERE id = ?")
        .bind(body.card_id)
        .fetch_one(&state.pool)
        .await
        .map_err(internal_error)?;

    Ok(Json(
        card_response_from_row(&state.pool, &card)
            .await
            .map_err(internal_error)?,
    ))
}

async fn block_card(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<BlockRequest>,
) -> Result<Json<CardResponse>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_cards() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    db::set_blocked(&state.pool, body.card_id, body.blocked)
        .await
        .map_err(internal_error)?;

    let card = sqlx::query_as::<_, db::CardRow>("SELECT * FROM cards WHERE id = ?")
        .bind(body.card_id)
        .fetch_one(&state.pool)
        .await
        .map_err(internal_error)?;

    Ok(Json(
        card_response_from_row(&state.pool, &card)
            .await
            .map_err(internal_error)?,
    ))
}

async fn update_card(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<UpdateCardRequest>,
) -> Result<Json<CardResponse>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_cards() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    db::update_card_info(
        &state.pool,
        id,
        body.first_name.as_deref(),
        body.last_name.as_deref(),
        body.company.as_deref(),
        body.phone.as_deref(),
    )
    .await
    .map_err(internal_error)?;

    let card = sqlx::query_as::<_, db::CardRow>("SELECT * FROM cards WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Card not found"})),
            )
        })?;

    Ok(Json(
        card_response_from_row(&state.pool, &card)
            .await
            .map_err(internal_error)?,
    ))
}

async fn card_transactions(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i64>,
    Query(params): Query<TransactionsQuery>,
) -> Result<Json<Vec<TransactionResponse>>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_cards() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    let txns = transactions::list_transactions_for_card_paginated(
        &state.pool,
        id,
        params.limit,
        params.before.as_deref(),
    )
    .await
    .map_err(internal_error)?;

    Ok(Json(
        txns.into_iter()
            .map(|t| TransactionResponse {
                id: t.id,
                card_id: t.card_id,
                amount: t.amount,
                action: t.action,
                created_at: t.created_at,
                service_name_sk: t.service_name_sk,
                service_name_en: t.service_name_en,
                service_kind: t.service_kind,
                valid_until: t.valid_until,
                deleted_at: t.deleted_at,
                note: t.note,
            })
            .collect(),
    ))
}

async fn card_stats(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<StatsResponse>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_cards() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    // Build the IN-clause placeholders dynamically from the constants. With
    // 2 entries today this is a 2-placeholder string; if CLASS_VISIT_NAMES_EN
    // grows (e.g. add HIIT), no SQL change is needed.
    let placeholders: String = std::iter::repeat_n("?", CLASS_VISIT_NAMES_EN.len())
        .collect::<Vec<_>>()
        .join(",");
    let visit_filter_sql =
        format!("service_id IN (SELECT id FROM services WHERE name_en IN ({placeholders}))");

    // ── Totals: one row, six numbers, three time windows ────────────────
    //
    // Note on `ELSE 0.0` (not `ELSE 0`): SQLite's SUM result type is REAL
    // only when at least one input row is REAL. When every row falls into
    // the `ELSE` branch (no matching topup rows), `ELSE 0` would produce
    // an INTEGER SUM that sqlx::Decode<f64> rejects. `0.0` keeps the SUM
    // typed as REAL even on the all-skip path.
    let totals_sql = format!(
        "SELECT
            COALESCE(SUM(CASE WHEN {visit_filter} AND deleted_at IS NULL
                              AND strftime('%Y-%m', created_at, 'localtime') =
                                  strftime('%Y-%m','now','localtime')
                         THEN 1 ELSE 0 END), 0) AS visits_month,
            COALESCE(SUM(CASE WHEN action='topup' AND amount > 0 AND deleted_at IS NULL
                              AND strftime('%Y-%m', created_at, 'localtime') =
                                  strftime('%Y-%m','now','localtime')
                         THEN amount ELSE 0.0 END), 0.0) AS topup_month,
            COALESCE(SUM(CASE WHEN {visit_filter} AND deleted_at IS NULL
                              AND strftime('%Y',    created_at, 'localtime') =
                                  strftime('%Y','now','localtime')
                         THEN 1 ELSE 0 END), 0) AS visits_year,
            COALESCE(SUM(CASE WHEN action='topup' AND amount > 0 AND deleted_at IS NULL
                              AND strftime('%Y',    created_at, 'localtime') =
                                  strftime('%Y','now','localtime')
                         THEN amount ELSE 0.0 END), 0.0) AS topup_year,
            COALESCE(SUM(CASE WHEN {visit_filter} AND deleted_at IS NULL
                         THEN 1 ELSE 0 END), 0) AS visits_all,
            COALESCE(SUM(CASE WHEN action='topup' AND amount > 0 AND deleted_at IS NULL
                         THEN amount ELSE 0.0 END), 0.0) AS topup_all
         FROM transactions
         WHERE card_id = ?",
        visit_filter = visit_filter_sql
    );

    let mut totals_q = sqlx::query_as::<_, (i64, f64, i64, f64, i64, f64)>(&totals_sql);
    // The visit-filter sub-clause appears 3 times (month / year / all). Bind
    // the class-name placeholders 3 times, in the same order.
    for _ in 0..3 {
        for n in CLASS_VISIT_NAMES_EN {
            totals_q = totals_q.bind(*n);
        }
    }
    totals_q = totals_q.bind(id);
    let (vm, tm, vy, ty, va, ta) = totals_q
        .fetch_one(&state.pool)
        .await
        .map_err(internal_error)?;

    // ── Monthly buckets: 12 rows aligned to the last 12 calendar months ─
    let now = chrono::Local::now();
    let mut labels: Vec<String> = Vec::with_capacity(12);
    for i in (0..12).rev() {
        let mut year = now.year();
        let mut month = now.month() as i32 - i;
        while month < 1 {
            month += 12;
            year -= 1;
        }
        labels.push(format!("{:04}-{:02}", year, month));
    }
    let oldest_label = labels.first().unwrap().clone();
    // `ELSE 0.0` for topped_up: same reason as the totals query — keeps the
    // column typed as REAL even when a month has no topup rows.
    let bucket_sql = format!(
        "SELECT
            strftime('%Y-%m', created_at, 'localtime') AS ym,
            SUM(CASE WHEN {visit_filter} THEN 1 ELSE 0 END) AS visits,
            SUM(CASE WHEN action='topup' AND amount > 0 THEN amount ELSE 0.0 END) AS topped_up
         FROM transactions
         WHERE card_id = ?
           AND deleted_at IS NULL
           AND strftime('%Y-%m', created_at, 'localtime') >= ?
         GROUP BY ym",
        visit_filter = visit_filter_sql
    );
    let mut bucket_q = sqlx::query_as::<_, (String, i64, f64)>(&bucket_sql);
    for n in CLASS_VISIT_NAMES_EN {
        bucket_q = bucket_q.bind(*n);
    }
    bucket_q = bucket_q.bind(id).bind(&oldest_label);
    let bucket_rows: Vec<(String, i64, f64)> = bucket_q
        .fetch_all(&state.pool)
        .await
        .map_err(internal_error)?;

    let monthly: Vec<MonthlyBucket> = labels
        .into_iter()
        .map(|ym| {
            let row = bucket_rows.iter().find(|r| r.0 == ym);
            MonthlyBucket {
                visits: row.map(|r| r.1).unwrap_or(0),
                topped_up_eur: row.map(|r| r.2).unwrap_or(0.0),
                year_month: ym,
            }
        })
        .collect();

    Ok(Json(StatsResponse {
        totals: PeriodTotals {
            this_month: PeriodAgg {
                visits: vm,
                topped_up_eur: tm,
            },
            this_year: PeriodAgg {
                visits: vy,
                topped_up_eur: ty,
            },
            all_time: PeriodAgg {
                visits: va,
                topped_up_eur: ta,
            },
        },
        monthly,
    }))
}

async fn my_balance(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<BalanceResponse>, (StatusCode, Json<serde_json::Value>)> {
    let rows = db::get_cards_with_pass_by_user(&state.pool, claims.sub)
        .await
        .map_err(internal_error)?;

    let txns = transactions::list_transactions_for_user(&state.pool, claims.sub)
        .await
        .map_err(internal_error)?;

    let card_responses = rows
        .iter()
        .map(|(c, pass, _last_visit)| card_response_from_row_with_pass(c, *pass, None))
        .collect();

    Ok(Json(BalanceResponse {
        cards: card_responses,
        transactions: txns
            .into_iter()
            .map(|t| TransactionResponse {
                id: t.id,
                card_id: t.card_id,
                amount: t.amount,
                action: t.action,
                created_at: t.created_at,
                service_name_sk: t.service_name_sk,
                service_name_en: t.service_name_en,
                service_kind: t.service_kind,
                valid_until: t.valid_until,
                deleted_at: t.deleted_at,
                note: t.note,
            })
            .collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_search_limit_is_ten() {
        // Pinning this constant: the dashboard dropdown is designed around
        // 10 suggestions. Any drift (0, 1, -1, larger) changes UX noticeably.
        assert_eq!(default_search_limit(), 10);
    }
}
