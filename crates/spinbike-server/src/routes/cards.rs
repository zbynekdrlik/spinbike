use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::db::{cards as db, transactions};
use crate::AppState;

#[derive(Deserialize)]
pub struct LinkCardRequest {
    pub barcode: String,
}

#[derive(Deserialize)]
pub struct ActivateCardRequest {
    pub barcode: String,
    pub initial_credit: f64,
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

#[derive(Serialize)]
pub struct CardResponse {
    pub id: i64,
    pub barcode: String,
    pub user_id: Option<i64>,
    pub blocked: bool,
    pub credit: f64,
    pub allow_debit: bool,
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
}

impl From<&db::CardRow> for CardResponse {
    fn from(c: &db::CardRow) -> Self {
        CardResponse {
            id: c.id,
            barcode: c.barcode.clone(),
            user_id: c.user_id,
            blocked: c.blocked != 0,
            credit: c.credit,
            allow_debit: c.allow_debit != 0,
        }
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/cards/link", post(link_card))
        .route("/api/cards/lookup/{barcode}", get(lookup_card))
        .route("/api/cards/activate", post(activate_card))
        .route("/api/cards/topup", post(topup_card))
        .route("/api/cards/block", post(block_card))
        .route("/api/my/balance", get(my_balance))
}

async fn link_card(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<LinkCardRequest>,
) -> Result<Json<CardResponse>, (StatusCode, Json<serde_json::Value>)> {
    let card = db::get_card_by_barcode(&state.pool, &body.barcode)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?
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
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;

    let updated = db::get_card_by_barcode(&state.pool, &body.barcode)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?
        .unwrap();

    Ok(Json(CardResponse::from(&updated)))
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
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Card not found"})),
            )
        })?;

    Ok(Json(CardResponse::from(&card)))
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

    let card_id = db::create_card(&state.pool, &body.barcode).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
    })?;

    if body.initial_credit > 0.0 {
        db::update_credit(&state.pool, card_id, body.initial_credit)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": e.to_string()})),
                )
            })?;

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
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;
    }

    let card = db::get_card_by_barcode(&state.pool, &body.barcode)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?
        .unwrap();

    Ok((StatusCode::CREATED, Json(CardResponse::from(&card))))
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

    db::update_credit(&state.pool, body.card_id, body.amount)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;

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
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
    })?;

    // Re-fetch the card to return updated state.
    let card = sqlx::query_as::<_, db::CardRow>("SELECT * FROM cards WHERE id = ?")
        .bind(body.card_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;

    Ok(Json(CardResponse::from(&card)))
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
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;

    let card = sqlx::query_as::<_, db::CardRow>("SELECT * FROM cards WHERE id = ?")
        .bind(body.card_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;

    Ok(Json(CardResponse::from(&card)))
}

async fn my_balance(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<BalanceResponse>, (StatusCode, Json<serde_json::Value>)> {
    let cards = db::get_card_by_user(&state.pool, claims.sub)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;

    let txns = transactions::list_transactions_for_user(&state.pool, claims.sub)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;

    Ok(Json(BalanceResponse {
        cards: cards.iter().map(CardResponse::from).collect(),
        transactions: txns
            .into_iter()
            .map(|t| TransactionResponse {
                id: t.id,
                card_id: t.card_id,
                amount: t.amount,
                action: t.action,
                created_at: t.created_at,
            })
            .collect(),
    }))
}
