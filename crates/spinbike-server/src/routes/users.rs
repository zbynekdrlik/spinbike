use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post, put},
};
use chrono::Datelike;
use serde::{Deserialize, Serialize};

use spinbike_core::auth::Role;
use spinbike_core::services::CLASS_VISIT_NAMES_EN;
use spinbike_core::stats::{MonthlyBucket, PeriodAgg, PeriodTotals, StatsResponse};

use crate::AppState;
use crate::auth::{AuthUser, StaffUser};
use crate::db::transactions::NOTE_MAX_CHARS;
use crate::db::{login_tokens, transactions, users as db};
use crate::error::ApiError;
use crate::mail::MailError;
use crate::routes::internal_error;
use spinbike_core::errors::ErrorCode;

#[derive(Serialize, Clone)]
pub struct UserResponse {
    pub id: i64,
    pub email: Option<String>,
    pub name: String,
    pub phone: Option<String>,
    pub company: Option<String>,
    pub card_code: Option<String>,
    pub credit: f64,
    pub blocked: bool,
    pub allow_debit: bool,
    pub allow_self_entry: bool,
    /// Typed role. Serializes to the SAME lowercase string the raw DB role
    /// produced (`Role`'s `#[serde(rename_all = "lowercase")]`), so the JSON
    /// wire format is byte-identical; deserialization gains the `Unknown`
    /// forward-compat fallback.
    pub role: Role,
    pub last_visit_at: Option<String>,
    pub pass: Option<CardPass>,
}

#[derive(Serialize, Clone)]
pub struct CardPass {
    pub valid_until: chrono::NaiveDate,
    pub days_remaining: i32,
    pub transaction_id: i64,
}

#[derive(Serialize, Clone)]
pub struct NegativeBalanceUserResponse {
    pub id: i64,
    pub card_code: Option<String>,
    pub credit: f64,
    pub blocked: bool,
    pub name: String,
    pub email: Option<String>,
    pub company: Option<String>,
    pub last_visit_at: Option<String>,
    pub pass: Option<CardPass>,
}

#[derive(Serialize)]
pub struct BalanceResponse {
    pub user_id: i64,
    pub name: String,
    pub credit: f64,
    pub card_code: Option<String>,
    pub allow_self_entry: bool,
    /// SQLite UTC timestamp; `None` = no active monthly pass.
    pub monthly_pass_active_until: Option<String>,
    /// Last 20 transactions for this user, newest first.
    pub recent: Vec<RecentTx>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct RecentTx {
    pub id: i64,
    pub created_at: String,
    pub action: String,
    pub amount: f64,
    pub valid_until: Option<String>,
    pub note: Option<String>,
    /// Joined from services (#147) — None when the transaction wasn't tied
    /// to a service (e.g. a plain top-up). Same join as
    /// `db::transactions::list_transactions_for_user_paginated`, used by the
    /// admin transactions list.
    pub service_name_sk: Option<String>,
    pub service_name_en: Option<String>,
}

#[derive(Serialize)]
pub struct TransactionResponse {
    pub id: i64,
    pub user_id: Option<i64>,
    pub amount: f64,
    pub action: String,
    pub created_at: String,
    pub service_name_sk: Option<String>,
    pub service_name_en: Option<String>,
    pub service_kind: Option<String>,
    pub valid_until: Option<chrono::NaiveDate>,
    pub deleted_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
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

#[derive(Deserialize)]
pub struct CreateUserRequest {
    pub name: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub company: Option<String>,
    #[serde(default)]
    pub card_code: Option<String>,
    #[serde(default)]
    pub initial_credit: Option<f64>,
}

#[derive(Deserialize)]
pub struct UpdateUserRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub company: Option<String>,
    #[serde(default)]
    pub card_code: Option<String>,
    #[serde(default)]
    pub allow_self_entry: Option<bool>,
    /// Plain-text password. Hashed server-side via argon2 before storage.
    /// Admin can set any user's password; customer can set OWN password
    /// (caller.sub == path id); staff is forbidden from resetting passwords
    /// for any user other than themselves.
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Deserialize)]
pub struct TopupRequest {
    pub user_id: i64,
    pub amount: f64,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Deserialize)]
pub struct BlockRequest {
    pub user_id: i64,
    pub blocked: bool,
}

// Replaces `impl From<&db::UserRow> for UserResponse`.
// Used for single-user handlers (lookup, create, topup, block, update)
// where we need a fresh DB query for the pass.
async fn user_response_from_row(
    pool: &sqlx::SqlitePool,
    u: &db::UserRow,
) -> anyhow::Result<UserResponse> {
    let pass = db::get_user_pass_tx(pool, u.id).await?;
    Ok(user_response_from_row_with_pass(u, pass, None))
}

/// Build a UserResponse from a pre-fetched pass (tx id + date) and last visit timestamp —
/// avoids per-user DB round-trip. Used by list_users and search_users which retrieve
/// pass info and last_visit_at in a single query.
fn user_response_from_row_with_pass(
    u: &db::UserRow,
    pass: Option<(i64, chrono::NaiveDate)>,
    last_visit_at: Option<String>,
) -> UserResponse {
    let today = chrono::Local::now().date_naive();
    let pass = pass.map(|(tx_id, d)| CardPass {
        valid_until: d,
        days_remaining: (d - today).num_days() as i32,
        transaction_id: tx_id,
    });
    UserResponse {
        id: u.id,
        email: u.email.clone(),
        name: u.name.clone(),
        phone: u.phone.clone(),
        company: u.company.clone(),
        card_code: u.card_code.clone(),
        credit: u.credit,
        blocked: u.blocked,
        allow_debit: u.allow_debit,
        allow_self_entry: u.allow_self_entry,
        role: Role::from(u.role.as_str()),
        last_visit_at,
        pass,
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/users", get(list_users).post(create_user))
        .route("/api/users/search", get(search_users))
        .route("/api/users/lookup/{code}", get(lookup_user))
        .route("/api/users/topup", post(topup_user))
        .route("/api/users/block", post(block_user))
        .route("/api/users/negative-balance", get(negative_balance))
        .route("/api/users/by-last-movement", get(by_last_movement))
        .route(
            "/api/users/{id}",
            put(update_user).delete(delete_user_route),
        )
        .route("/api/users/{id}/invite", post(invite_user))
        .route("/api/users/{id}/transactions", get(user_transactions))
        .route("/api/users/{id}/stats", get(user_stats))
        .route("/api/my/balance", get(my_balance))
}

async fn list_users(
    State(state): State<AppState>,
    _: StaffUser,
) -> Result<Json<Vec<UserResponse>>, ApiError> {
    let rows = db::list_all_users_with_pass(&state.pool)
        .await
        .map_err(internal_error)?;
    let out = rows
        .into_iter()
        .map(|(u, pass, last_visit)| user_response_from_row_with_pass(&u, pass, last_visit))
        .collect();
    Ok(Json(out))
}

async fn search_users(
    State(state): State<AppState>,
    _: StaffUser,
    Query(params): Query<SearchQuery>,
) -> Result<Json<Vec<UserResponse>>, ApiError> {
    let limit = params.limit.clamp(1, 50);
    let rows = db::search_users_with_pass(&state.pool, &params.q, limit)
        .await
        .map_err(internal_error)?;
    let out = rows
        .into_iter()
        .map(|(u, pass, last_visit)| user_response_from_row_with_pass(&u, pass, last_visit))
        .collect();
    Ok(Json(out))
}

async fn create_user(
    State(state): State<AppState>,
    StaffUser(claims): StaffUser,
    Json(body): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<UserResponse>), ApiError> {
    let name = body.name.trim().to_owned();
    if name.is_empty() {
        return Err(super::bad_request("Name must not be empty"));
    }

    // Normalise blank optional strings to None so the partial unique index
    // on card_code (WHERE card_code IS NOT NULL) does not collide on "" + ""
    // and so empty email strings don't become collision candidates.
    let body_email = body
        .email
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let body_card_code = body
        .card_code
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    if let Some(email) = body_email {
        if !email.contains('@') || !email.contains('.') {
            return Err(super::bad_request("Invalid email address"));
        }
        // Name the colliding account. This endpoint is staff/admin-only
        // (can_manage_cards guard above), so there is no customer to leak an
        // identity to — the name/card always go out.
        if let Some(existing) = db::get_user_by_email(&state.pool, email)
            .await
            .map_err(internal_error)?
        {
            return Err(ApiError::conflict_extra(
                ErrorCode::EmailConflict,
                serde_json::json!({
                    "conflict_name": existing.name,
                    "conflict_card": existing.card_code,
                }),
            ));
        }
    }

    if let Some(code) = body_card_code
        && db::get_user_by_card_code(&state.pool, code)
            .await
            .map_err(internal_error)?
            .is_some()
    {
        return Err(ApiError::conflict(ErrorCode::CardCodeConflict));
    }

    let body_phone = body
        .phone
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let body_company = body
        .company
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let user_id = db::create_user(
        &state.pool,
        body_email,
        None,
        &name,
        body_phone,
        body_company,
        body_card_code,
        "customer",
        body.initial_credit,
        None,
        None,
    )
    .await
    .map_err(|e| {
        if matches!(e, crate::db::DbError::UniqueViolation) {
            ApiError::conflict(ErrorCode::EmailOrCardConflict)
        } else {
            internal_error(e)
        }
    })?;

    if let Some(credit) = body.initial_credit.filter(|&c| c > 0.0) {
        transactions::create_transaction(
            &state.pool,
            Some(user_id),
            Some(claims.sub),
            None,
            credit,
            "topup",
            None,
        )
        .await
        .map_err(internal_error)?;
    }

    let user = db::get_user_by_id(&state.pool, user_id)
        .await
        .map_err(internal_error)?
        .unwrap();

    Ok((
        StatusCode::CREATED,
        Json(
            user_response_from_row(&state.pool, &user)
                .await
                .map_err(internal_error)?,
        ),
    ))
}

#[derive(Serialize)]
struct InviteResponse {
    sent_to: String,
    /// Present only in `SMTP_TEST_MODE=capture` so E2E can drive the welcome
    /// flow without a real mailbox. Never populated with a real SMTP relay.
    #[serde(skip_serializing_if = "Option::is_none")]
    test_link: Option<String>,
}

/// Compose the unaccented-Slovak invite email. Returns (subject, text, html).
fn invite_email(link: &str) -> (String, String, String) {
    let subject = "Vitaj v SpinBike".to_string();
    let text = format!(
        "Ahoj,\n\nSpinBike je nasa aplikacia na rezervacie spinningu a spravu kreditu. \
         Klikni na odkaz a aktivuj si pristup:\n{link}\n\nOdkaz plati 14 dni."
    );
    let html = format!(
        "<p>Ahoj,</p>\
         <p>SpinBike je nasa aplikacia na rezervacie spinningu a spravu kreditu.</p>\
         <p>Klikni na odkaz a aktivuj si pristup:</p>\
         <p><a href=\"{link}\">Aktivovat pristup</a></p>\
         <p>Odkaz plati 14 dni.</p>"
    );
    (subject, text, html)
}

/// Admin/staff-only: email a magic invite link to the given user. Requires the
/// user to have an email. Returns 503 `mail_not_configured` when the mail
/// module is Disabled (missing SMTP env).
async fn invite_user(
    State(state): State<AppState>,
    _: StaffUser,
    Path(id): Path<i64>,
) -> Result<Json<InviteResponse>, ApiError> {
    let user = db::get_user_by_id(&state.pool, id)
        .await
        .map_err(internal_error)?
        .ok_or(ApiError::NotFound(ErrorCode::UserNotFound))?;
    if user.deleted_at.is_some() {
        return Err(ApiError::NotFound(ErrorCode::UserNotFound));
    }

    let email = match user
        .email
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(e) => e.to_string(),
        None => {
            return Err(super::bad_request(
                "User has no email address to send an invite to",
            ));
        }
    };

    let raw = login_tokens::create_token(
        &state.pool,
        id,
        login_tokens::PURPOSE_INVITE,
        login_tokens::INVITE_TTL_SECS,
    )
    .await
    .map_err(internal_error)?;

    let link = format!(
        "{}/welcome?t={}",
        state.public_base_url.trim_end_matches('/'),
        raw
    );
    let (subject, text, html) = invite_email(&link);

    match state.mail.send(&email, &subject, &text, &html).await {
        Ok(()) => {}
        Err(MailError::Disabled) => {
            tracing::warn!(user_id = id, "invite: mail is disabled — returning 503");
            return Err(ApiError::ServiceUnavailable(ErrorCode::MailNotConfigured));
        }
        Err(e) => return Err(internal_error(e)),
    }

    // last_captured() is Some only in capture test mode — use it as the
    // capture-mode detector, then echo the freshly-composed link.
    let test_link = state.mail.last_captured().map(|_| link);
    tracing::info!(user_id = id, %email, "invite: sent");
    Ok(Json(InviteResponse {
        sent_to: email,
        test_link,
    }))
}

async fn lookup_user(
    State(state): State<AppState>,
    _: StaffUser,
    Path(code): Path<String>,
) -> Result<Json<UserResponse>, ApiError> {
    let user = db::get_user_by_card_code(&state.pool, &code)
        .await
        .map_err(internal_error)?
        .ok_or(ApiError::NotFound(ErrorCode::UserNotFound))?;

    Ok(Json(
        user_response_from_row(&state.pool, &user)
            .await
            .map_err(internal_error)?,
    ))
}

async fn topup_user(
    State(state): State<AppState>,
    StaffUser(claims): StaffUser,
    Json(body): Json<TopupRequest>,
) -> Result<Json<UserResponse>, ApiError> {
    if body.amount <= 0.0 {
        return Err(super::bad_request("Amount must be greater than zero"));
    }
    if let Some(n) = body.note.as_deref()
        && n.chars().count() > NOTE_MAX_CHARS
    {
        return Err(super::bad_request("Note must be 200 characters or fewer"));
    }
    let note_for_db = body.note.as_deref().filter(|s| !s.trim().is_empty());

    let user = db::get_user_by_id(&state.pool, body.user_id)
        .await
        .map_err(internal_error)?
        .ok_or(ApiError::NotFound(ErrorCode::UserNotFound))?;

    if user.deleted_at.is_some() {
        return Err(ApiError::NotFound(ErrorCode::UserNotFound));
    }

    if user.blocked {
        return Err(ApiError::Forbidden(ErrorCode::UserBlocked));
    }

    db::update_credit(&state.pool, body.user_id, body.amount)
        .await
        .map_err(internal_error)?;

    transactions::create_transaction(
        &state.pool,
        Some(body.user_id),
        Some(claims.sub),
        None,
        body.amount,
        "topup",
        note_for_db,
    )
    .await
    .map_err(internal_error)?;

    let updated = db::get_user_by_id(&state.pool, body.user_id)
        .await
        .map_err(internal_error)?
        .unwrap();

    Ok(Json(
        user_response_from_row(&state.pool, &updated)
            .await
            .map_err(internal_error)?,
    ))
}

async fn block_user(
    State(state): State<AppState>,
    _: StaffUser,
    Json(body): Json<BlockRequest>,
) -> Result<Json<UserResponse>, ApiError> {
    // Verify user is active before mutating — soft-deleted users are
    // invariant-frozen (#56).
    let existing = db::get_user_by_id(&state.pool, body.user_id)
        .await
        .map_err(internal_error)?
        .ok_or(ApiError::NotFound(ErrorCode::UserNotFound))?;
    if existing.deleted_at.is_some() {
        return Err(ApiError::NotFound(ErrorCode::UserNotFound));
    }

    db::set_blocked(&state.pool, body.user_id, body.blocked)
        .await
        .map_err(internal_error)?;

    let user = db::get_user_by_id(&state.pool, body.user_id)
        .await
        .map_err(internal_error)?
        .ok_or(ApiError::NotFound(ErrorCode::UserNotFound))?;

    Ok(Json(
        user_response_from_row(&state.pool, &user)
            .await
            .map_err(internal_error)?,
    ))
}

async fn negative_balance(
    State(state): State<AppState>,
    _: StaffUser,
) -> Result<Json<Vec<NegativeBalanceUserResponse>>, ApiError> {
    let rows = db::list_negative_balance(&state.pool)
        .await
        .map_err(internal_error)?;
    let today = chrono::Local::now().date_naive();
    let out = rows
        .into_iter()
        .map(|r| {
            let pass = match (r.pass_tx_id, r.pass_valid_until) {
                (Some(tx_id), Some(valid_until)) => Some(CardPass {
                    valid_until,
                    days_remaining: (valid_until - today).num_days() as i32,
                    transaction_id: tx_id,
                }),
                _ => None,
            };
            NegativeBalanceUserResponse {
                id: r.id,
                card_code: r.card_code,
                credit: r.credit,
                blocked: r.blocked,
                name: r.name,
                email: r.email,
                company: r.company,
                last_visit_at: r.last_visit_at,
                pass,
            }
        })
        .collect();
    Ok(Json(out))
}

async fn update_user(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<UpdateUserRequest>,
) -> Result<Json<UserResponse>, ApiError> {
    // Self-edit is allowed regardless of role: a customer can update their
    // own name/email/phone/company/password on their own user row. Field-
    // level guards below still enforce admin-only for `allow_self_entry` and
    // admin-or-self for `password`. Staff/admin keep their existing
    // permissions for editing OTHER users.
    let is_self = claims.sub == id;
    let is_staff_or_admin = claims.role.can_manage_cards();
    tracing::info!(
        caller_id = claims.sub,
        caller_role = ?claims.role,
        target_id = id,
        has_name = body.name.is_some(),
        has_email = body.email.is_some(),
        has_phone = body.phone.is_some(),
        has_company = body.company.is_some(),
        has_card_code = body.card_code.is_some(),
        has_allow_self_entry = body.allow_self_entry.is_some(),
        has_password = body.password.is_some(),
        "PUT /api/users/{id}: update request"
    );
    if !is_staff_or_admin && !is_self {
        return Err(ApiError::Forbidden(ErrorCode::StaffRequired));
    }

    // card_code is the legacy-barcode identifier used by staff search/scan
    // workflows. Only staff/admin can change it — customers cannot rewrite
    // their own card_code (would let them collide with other users' codes or
    // claim a freshly-typed code).
    if body.card_code.is_some() && !is_staff_or_admin {
        return Err(ApiError::Forbidden(ErrorCode::CardCodeStaffOnly));
    }

    // Soft-deleted users are invariant-frozen (#56) — reject mutation upfront.
    let target = db::get_user_by_id(&state.pool, id)
        .await
        .map_err(internal_error)?
        .ok_or(ApiError::NotFound(ErrorCode::UserNotFound))?;
    if target.deleted_at.is_some() {
        return Err(ApiError::NotFound(ErrorCode::UserNotFound));
    }

    if let Some(ref email) = body.email {
        if !email.contains('@') || !email.contains('.') {
            return Err(super::bad_request("Invalid email address"));
        }
        // Collision check: another user already has this email.
        if let Some(existing) = db::get_user_by_email(&state.pool, email)
            .await
            .map_err(internal_error)?
            && existing.id != id
        {
            // Name the colliding account so a STAFF/ADMIN operator can go find
            // and fix it (the reported need: the CEO just wants to see "email
            // is already used by so-and-so", then he sorts it out himself).
            // SECURITY: never leak another account's identity to a CUSTOMER
            // self-editing their own row — that would turn the email field
            // into a name-enumeration oracle. Customers get only the generic
            // message; the name/card go out solely to staff/admin.
            let mut extra = serde_json::Map::new();
            if is_staff_or_admin {
                extra.insert("conflict_name".into(), serde_json::json!(existing.name));
                extra.insert(
                    "conflict_card".into(),
                    serde_json::json!(existing.card_code),
                );
            }
            return Err(if extra.is_empty() {
                ApiError::conflict(ErrorCode::EmailConflict)
            } else {
                ApiError::conflict_extra(ErrorCode::EmailConflict, serde_json::Value::Object(extra))
            });
        }
    }

    if let Some(ref code) = body.card_code
        && let Some(existing) = db::get_user_by_card_code(&state.pool, code)
            .await
            .map_err(internal_error)?
        && existing.id != id
    {
        return Err(ApiError::conflict(ErrorCode::CardCodeConflict));
    }

    db::update_user_info(
        &state.pool,
        id,
        body.name.as_deref(),
        body.email.as_deref(),
        body.phone.as_deref(),
        body.company.as_deref(),
        body.card_code.as_deref(),
    )
    .await
    .map_err(internal_error)?;

    if let Some(allow) = body.allow_self_entry {
        if claims.role != spinbike_core::auth::Role::Admin {
            return Err(ApiError::Forbidden(ErrorCode::AllowSelfEntryAdminOnly));
        }
        db::update_user_allow_self_entry(&state.pool, id, allow)
            .await
            .map_err(internal_error)?;
    }

    if let Some(ref pw) = body.password {
        // Admin can set any user's password.
        // Anyone (incl. customer / staff) can set their OWN password.
        // Staff CANNOT reset another user's password.
        let is_admin = claims.role == spinbike_core::auth::Role::Admin;
        let is_self = claims.sub == id;
        if !is_admin && !is_self {
            return Err(ApiError::Forbidden(ErrorCode::PasswordAdminOnly));
        }
        if pw.len() < 8 {
            return Err(super::bad_request("Password must be at least 8 characters"));
        }
        let hash = crate::auth::hash_password(pw).map_err(internal_error)?;
        db::update_user_password_hash(&state.pool, id, &hash)
            .await
            .map_err(internal_error)?;
    }

    let user = db::get_user_by_id(&state.pool, id)
        .await
        .map_err(internal_error)?
        .ok_or(ApiError::NotFound(ErrorCode::UserNotFound))?;

    Ok(Json(
        user_response_from_row(&state.pool, &user)
            .await
            .map_err(internal_error)?,
    ))
}

async fn user_transactions(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(id): Path<i64>,
    Query(params): Query<TransactionsQuery>,
) -> Result<Json<Vec<TransactionResponse>>, ApiError> {
    // Staff can see any user's transactions; a customer can only see their own.
    // Ownership-mixed guard: staff can view anyone's transactions, a customer
    // only their own (claims.sub == id). Needs both the role AND claims.sub, so
    // it can't move to a pure role extractor and stays inline on AuthUser.
    if !claims.role.can_manage_cards() && claims.sub != id {
        return Err(ApiError::Forbidden(ErrorCode::StaffRequired));
    }

    let txns = transactions::list_transactions_for_user_paginated(
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
                user_id: t.user_id,
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

async fn user_stats(
    State(state): State<AppState>,
    _: StaffUser,
    Path(id): Path<i64>,
) -> Result<Json<StatsResponse>, ApiError> {
    // Build the IN-clause placeholders dynamically from the constants.
    let placeholders: String = std::iter::repeat_n("?", CLASS_VISIT_NAMES_EN.len())
        .collect::<Vec<_>>()
        .join(",");
    let visit_filter_sql =
        format!("service_id IN (SELECT id FROM services WHERE name_en IN ({placeholders}))");

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
         WHERE user_id = ?",
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
    let bucket_sql = format!(
        "SELECT
            strftime('%Y-%m', created_at, 'localtime') AS ym,
            SUM(CASE WHEN {visit_filter} THEN 1 ELSE 0 END) AS visits,
            SUM(CASE WHEN action='topup' AND amount > 0 THEN amount ELSE 0.0 END) AS topped_up
         FROM transactions
         WHERE user_id = ?
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

#[derive(serde::Deserialize)]
struct ByMovementQuery {
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
}
fn default_limit() -> i64 {
    50
}

async fn by_last_movement(
    State(state): State<AppState>,
    _: StaffUser,
    Query(q): Query<ByMovementQuery>,
) -> Result<Json<Vec<db::UserByMovementRow>>, ApiError> {
    if !(1..=200).contains(&q.limit) || q.offset < 0 {
        return Err(super::bad_request("limit must be 1..=200, offset >= 0"));
    }
    let rows = db::users_by_last_movement(&state.pool, q.limit, q.offset)
        .await
        .map_err(internal_error)?;
    Ok(Json(rows))
}

#[derive(serde::Serialize)]
struct DeleteUserResp {
    id: i64,
    deleted_at: String,
}

async fn delete_user_route(
    State(state): State<AppState>,
    _: StaffUser,
    Path(id): Path<i64>,
) -> Result<Json<DeleteUserResp>, ApiError> {
    match db::delete_user(&state.pool, id)
        .await
        .map_err(internal_error)?
    {
        db::DeleteUserOutcome::Deleted { deleted_at } => {
            Ok(Json(DeleteUserResp { id, deleted_at }))
        }
        db::DeleteUserOutcome::NotFound => Err(ApiError::NotFound(ErrorCode::UserNotFound)),
        db::DeleteUserOutcome::AlreadyDeleted => {
            Err(ApiError::conflict(ErrorCode::UserAlreadyDeleted))
        }
    }
}

async fn my_balance(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<BalanceResponse>, ApiError> {
    let user_id = claims.sub;
    tracing::debug!(user_id, "my_balance: loading user row");

    // 1. User row — includes the new allow_self_entry column.
    // SQLite stores allow_self_entry as INTEGER (0/1); fetch as i64 here and
    // map to bool below — sqlx tuple destructuring is stricter about types
    // than `#[derive(FromRow)]`, so we avoid the bool type entirely at the
    // query boundary.
    let user_row: Option<(i64, String, f64, Option<String>, i64)> = sqlx::query_as(
        "SELECT id, name, credit, card_code, allow_self_entry \
         FROM users WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?;

    let (id, name, credit, card_code, allow_self_entry) = match user_row {
        Some((id, name, credit, card_code, ase)) => {
            // Admin/staff always see the door button enabled — they bypass
            // the per-user opt-in toggle (they manage the place). Stored
            // flag stays as-is; this is just the effective UI value.
            let role_is_staff_or_admin = matches!(
                claims.role,
                spinbike_core::auth::Role::Admin | spinbike_core::auth::Role::Staff
            );
            let effective_ase = ase != 0 || role_is_staff_or_admin;
            (id, name, credit, card_code, effective_ase)
        }
        None => {
            tracing::warn!(user_id, "my_balance: user not found or soft-deleted");
            return Err(ApiError::NotFound(ErrorCode::UserNotFound));
        }
    };

    // 2. Active monthly-pass valid_until, via the canonical `user_active_pass`
    //    view (migration V18) — the SAME definition the charger and the staff
    //    user lists use. The view already exposes the user's latest non-voided
    //    monthly-pass purchase; here we surface it only while it is still valid
    //    today or later (an expired pass shows as "no active pass"). The
    //    comparison is INCLUSIVE of the last paid day and coerces both sides to
    //    a calendar date (`date(valid_until) >= date('now')`), matching the
    //    charger and the door route (#179). The previous
    //    `valid_until > datetime('now')` compared a bare date against a
    //    datetime and, via SQLite's byte-wise TEXT ordering, wrongly reported
    //    "no active pass" from midnight of the pass's own last valid day.
    tracing::debug!(user_id, "my_balance: querying monthly_pass_active_until");
    let monthly_pass_active_until: Option<String> = sqlx::query_scalar(
        "SELECT valid_until FROM user_active_pass \
          WHERE user_id = ? AND date(valid_until) >= date('now')",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?;

    // 3. Last 20 transactions (newest first). LEFT JOIN services (#147) so
    //    the customer sees WHICH service a movement was for, same as the
    //    admin transactions list. RecentTx derives FromRow (column-name
    //    matched, not positional) so the aliases below just need to match
    //    its field names — no manual tuple destructuring to keep in sync.
    tracing::debug!(user_id, "my_balance: querying recent transactions");
    let recent: Vec<RecentTx> = sqlx::query_as::<_, RecentTx>(
        "SELECT t.id, t.created_at, t.action, t.amount, t.valid_until, t.note, \
                s.name_sk AS service_name_sk, s.name_en AS service_name_en \
           FROM transactions t \
           LEFT JOIN services s ON s.id = t.service_id \
          WHERE t.user_id = ? \
            AND t.deleted_at IS NULL \
          ORDER BY t.created_at DESC \
          LIMIT 20",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(internal_error)?;

    tracing::info!(
        user_id = id,
        credit,
        allow_self_entry,
        pass_active = monthly_pass_active_until.is_some(),
        recent_count = recent.len(),
        "my_balance: ok"
    );

    Ok(Json(BalanceResponse {
        user_id: id,
        name,
        credit,
        card_code,
        allow_self_entry,
        monthly_pass_active_until,
        recent,
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

    /// Wire-compat guard (#98): the typed `role` field MUST serialize to the
    /// exact lowercase string the previous `String` role produced, so the
    /// `/api/users*` JSON payload is byte-identical after the migration.
    #[test]
    fn user_response_serializes_role_to_lowercase_string() {
        for (role, expected) in [
            (Role::Admin, "admin"),
            (Role::Staff, "staff"),
            (Role::Customer, "customer"),
        ] {
            let resp = UserResponse {
                id: 1,
                email: Some("a@b.com".into()),
                name: "N".into(),
                phone: None,
                company: None,
                card_code: None,
                credit: 0.0,
                blocked: false,
                allow_debit: false,
                allow_self_entry: false,
                role,
                last_visit_at: None,
                pass: None,
            };
            let v = serde_json::to_value(&resp).unwrap();
            assert_eq!(v["role"], expected, "role must serialize to {expected:?}");
        }
    }
}
