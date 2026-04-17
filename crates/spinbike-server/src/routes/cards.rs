use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::auth::AuthUser;
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

#[derive(Serialize)]
pub struct CardPass {
    pub valid_until: chrono::NaiveDate,
    pub days_remaining: i32,
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
    // Name of the service the transaction paid for, when applicable.
    pub service_name: Option<String>,
    pub valid_until: Option<chrono::NaiveDate>,
}

// Replaces `impl From<&db::CardRow> for CardResponse`.
async fn card_response_from_row(
    pool: &sqlx::SqlitePool,
    c: &db::CardRow,
) -> anyhow::Result<CardResponse> {
    let today = chrono::Local::now().date_naive();
    let pass = db::get_card_pass_valid_until(pool, c.id)
        .await?
        .filter(|&d| d >= today)
        .map(|d| CardPass {
            valid_until: d,
            days_remaining: (d - today).num_days() as i32,
        });
    Ok(CardResponse {
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
    })
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
    let cards = db::search_cards(&state.pool, &params.q, limit)
        .await
        .map_err(internal_error)?;
    let mut out = Vec::with_capacity(cards.len());
    for c in &cards {
        out.push(
            card_response_from_row(&state.pool, c)
                .await
                .map_err(internal_error)?,
        );
    }
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
    let cards = db::list_all_cards(&state.pool)
        .await
        .map_err(internal_error)?;
    let mut out = Vec::with_capacity(cards.len());
    for c in &cards {
        out.push(
            card_response_from_row(&state.pool, c)
                .await
                .map_err(internal_error)?,
        );
    }
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
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Amount must be greater than zero"})),
        ));
    }

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
) -> Result<Json<Vec<TransactionResponse>>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_manage_cards() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    let txns = transactions::list_transactions_for_card(&state.pool, id)
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
                service_name: t.service_name,
                valid_until: t.valid_until,
            })
            .collect(),
    ))
}

async fn my_balance(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<BalanceResponse>, (StatusCode, Json<serde_json::Value>)> {
    let cards = db::get_card_by_user(&state.pool, claims.sub)
        .await
        .map_err(internal_error)?;

    let txns = transactions::list_transactions_for_user(&state.pool, claims.sub)
        .await
        .map_err(internal_error)?;

    let mut card_responses = Vec::with_capacity(cards.len());
    for c in &cards {
        card_responses.push(
            card_response_from_row(&state.pool, c)
                .await
                .map_err(internal_error)?,
        );
    }

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
                service_name: t.service_name,
                valid_until: t.valid_until,
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
