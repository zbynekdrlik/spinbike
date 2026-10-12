use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get},
};
use serde::Deserialize;

use crate::AppState;
use crate::auth::{AuthUser, StaffUser, require_live_session};
use crate::db::persistent_bookings::SlotRow;
use crate::error::ApiError;
use crate::routes::internal_error;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/users/{user_id}/persistent-bookings",
            get(list).post(create),
        )
        .route(
            "/api/users/{user_id}/persistent-bookings/{template_id}",
            delete(end_persistent),
        )
        // #382 R5: the customer manages their OWN weekly spinning slots.
        .route(
            "/api/my/persistent-bookings",
            get(my_slots).put(set_my_slots),
        )
}

#[derive(Deserialize)]
struct CreateReq {
    template_id: i64,
}

/// `PUT /api/my/persistent-bookings` body: the COMPLETE set of template ids
/// the customer wants to be booked into every week (an empty list ends
/// every subscription).
#[derive(Deserialize)]
struct SetSlotsReq {
    template_ids: Vec<i64>,
}

async fn list(
    State(state): State<AppState>,
    _: StaffUser,
    Path(user_id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let rows = crate::db::persistent_bookings::list_for_user(&state.pool, user_id)
        .await
        .map_err(internal_error)?;
    Ok(Json(serde_json::to_value(rows).unwrap()))
}

async fn create(
    State(state): State<AppState>,
    _: StaffUser,
    Path(user_id): Path<i64>,
    Json(body): Json<CreateReq>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let id = crate::db::persistent_bookings::create(&state.pool, user_id, body.template_id)
        .await
        .map_err(internal_error)?;

    // Materialise now so the card page immediately reflects AUTO rows.
    if let Err(e) = crate::jobs::materialiser::sweep(&state.pool).await {
        tracing::error!("post-create materialiser sweep failed: {e}");
    }

    Ok((StatusCode::CREATED, Json(serde_json::json!({"id": id}))))
}

async fn end_persistent(
    State(state): State<AppState>,
    _: StaffUser,
    Path((user_id, template_id)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    // End the subscription and cancel its future, still-cancellable (before
    // the class day's 09:00 cutoff, uncharged) bookings — the SAME shared
    // rule the customer's own PUT uses (#382 R5). Gym-local now, never the
    // UTC `datetime('now')` the old inline UPDATE compared against.
    crate::db::persistent_bookings::end_and_cancel_future(
        &state.pool,
        user_id,
        template_id,
        crate::util::now_bratislava(),
    )
    .await
    .map_err(internal_error)?;

    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/my/persistent-bookings` — every active class template with the
/// caller's `subscribed` flag (#382 R5).
async fn my_slots(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<Vec<SlotRow>>, ApiError> {
    require_live_session(&state.pool, claims.sub).await?;
    let slots = crate::db::persistent_bookings::list_slots_for_user(&state.pool, claims.sub)
        .await
        .map_err(internal_error)?;
    Ok(Json(slots))
}

/// `PUT /api/my/persistent-bookings` — set the caller's weekly slots to
/// exactly `template_ids` (#382 R5). Every id must be an ACTIVE class
/// template (400 otherwise — nothing is changed). Dropped slots end and
/// cancel their future uncharged bookings; new slots are materialised right
/// away so the customer's home card shows the bookings immediately. A full
/// class simply gets no booking for that date (the materialiser warns).
/// Responds with the updated slot list.
async fn set_my_slots(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<SetSlotsReq>,
) -> Result<Json<Vec<SlotRow>>, ApiError> {
    require_live_session(&state.pool, claims.sub).await?;

    let active: Vec<i64> = crate::db::classes::list_active_templates(&state.pool)
        .await
        .map_err(internal_error)?
        .into_iter()
        .map(|t| t.id)
        .collect();
    if let Some(bad) = body.template_ids.iter().find(|id| !active.contains(id)) {
        tracing::info!(
            user_id = claims.sub,
            template_id = *bad,
            "recurring slots rejected: not an active class template"
        );
        return Err(super::bad_request("Unknown or inactive class template"));
    }

    crate::db::persistent_bookings::set_subscriptions(
        &state.pool,
        claims.sub,
        &body.template_ids,
        crate::util::now_bratislava(),
    )
    .await
    .map_err(internal_error)?;

    // Materialise now (idempotent full sweep — the same call the staff POST
    // makes). A sweep failure must not undo the saved subscription: the
    // hourly sweep retries, so it is logged, not surfaced.
    match crate::jobs::materialiser::sweep(&state.pool).await {
        Ok(summary) => tracing::info!(
            user_id = claims.sub,
            created = summary.created,
            skipped_full = summary.skipped_full,
            "recurring slots saved; materialised"
        ),
        Err(e) => tracing::error!(
            user_id = claims.sub,
            "post-update materialiser sweep failed: {e}"
        ),
    }

    let slots = crate::db::persistent_bookings::list_slots_for_user(&state.pool, claims.sub)
        .await
        .map_err(internal_error)?;
    Ok(Json(slots))
}
