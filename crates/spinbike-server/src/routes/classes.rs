use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post},
};
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::auth::{AuthUser, OptionalAuthUser};
use crate::db::classes as db;
use crate::routes::internal_error;
use spinbike_core::ws::ServerMsg;

#[derive(Deserialize)]
pub struct ScheduleQuery {
    pub from: String,
    pub to: String,
}

#[derive(Serialize)]
pub struct ClassOccurrenceResponse {
    pub template_id: i64,
    pub date: String,
    pub weekday: i64,
    pub start_time: String,
    pub duration_minutes: i64,
    pub instructor_id: Option<i64>,
    pub capacity: i64,
    pub booked: i64,
    pub cancelled: bool,
    pub user_booked: bool,
    pub user_booking_id: Option<i64>,
    pub user_booking_source: Option<String>,
}

#[derive(Deserialize)]
pub struct BookingRequest {
    pub template_id: i64,
    pub date: String,
    pub user_id: Option<i64>,
    pub card_id: Option<i64>,
}

#[derive(Serialize)]
pub struct BookingResponse {
    pub id: i64,
    pub template_id: i64,
    pub date: String,
    pub user_id: i64,
}

/// A participant in a class (booking joined with user info).
#[derive(Serialize)]
pub struct ParticipantResponse {
    pub booking_id: i64,
    pub user_name: String,
    pub user_email: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/classes", get(list_classes))
        .route(
            "/api/classes/{template_id}/{date}/participants",
            get(list_participants),
        )
        .route("/api/bookings", post(create_booking))
        .route("/api/bookings/{id}", delete(cancel_booking))
        .route("/api/my/bookings", get(my_bookings))
}

async fn list_classes(
    State(state): State<AppState>,
    OptionalAuthUser(claims): OptionalAuthUser,
    Query(query): Query<ScheduleQuery>,
) -> Result<Json<Vec<ClassOccurrenceResponse>>, (StatusCode, Json<serde_json::Value>)> {
    let from = NaiveDate::parse_from_str(&query.from, "%Y-%m-%d").map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid 'from' date format, expected YYYY-MM-DD"})),
        )
    })?;
    let to = NaiveDate::parse_from_str(&query.to, "%Y-%m-%d").map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid 'to' date format, expected YYYY-MM-DD"})),
        )
    })?;

    let templates = db::list_active_templates(&state.pool)
        .await
        .map_err(internal_error)?;

    let mut occurrences = Vec::new();
    let mut current = from;

    while current <= to {
        // chrono weekday: Mon=0..Sun=6; our DB weekday: 0=Mon..6=Sun
        let weekday = current.weekday().num_days_from_monday() as i64;

        for tmpl in &templates {
            if tmpl.weekday != weekday {
                continue;
            }

            let date_str = current.format("%Y-%m-%d").to_string();

            let cancelled = db::is_occurrence_cancelled(&state.pool, tmpl.id, &date_str)
                .await
                .unwrap_or(false);

            let booked = db::get_booking_count(&state.pool, tmpl.id, &date_str)
                .await
                .unwrap_or(0);

            // Check if authenticated user has a booking for this class.
            let (user_booked, user_booking_id, user_booking_source) = if let Some(ref c) = claims {
                let bookings = db::list_bookings_for_class(&state.pool, tmpl.id, &date_str)
                    .await
                    .unwrap_or_default();
                let user_booking = bookings.iter().find(|b| b.user_id == c.sub);
                (
                    user_booking.is_some(),
                    user_booking.map(|b| b.id),
                    user_booking.map(|b| b.source.clone()),
                )
            } else {
                (false, None, None)
            };

            occurrences.push(ClassOccurrenceResponse {
                template_id: tmpl.id,
                date: date_str,
                weekday: tmpl.weekday,
                start_time: tmpl.start_time.clone(),
                duration_minutes: tmpl.duration_minutes,
                instructor_id: tmpl.instructor_id,
                capacity: tmpl.capacity,
                booked,
                cancelled,
                user_booked,
                user_booking_id,
                user_booking_source,
            });
        }

        current = current.succ_opt().unwrap_or(current);
        if current == from && to > from {
            // Safety: prevent infinite loop
            break;
        }
    }

    Ok(Json(occurrences))
}

/// Staff-only endpoint: list participants for a specific class occurrence.
async fn list_participants(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path((template_id, date)): Path<(i64, String)>,
) -> Result<Json<Vec<ParticipantResponse>>, (StatusCode, Json<serde_json::Value>)> {
    if !claims.role.can_cancel_any_booking() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Staff access required"})),
        ));
    }

    #[derive(sqlx::FromRow)]
    struct Row {
        booking_id: i64,
        user_name: String,
        user_email: String,
    }

    let rows = sqlx::query_as::<_, Row>(
        "SELECT b.id AS booking_id, u.name AS user_name, u.email AS user_email
         FROM bookings b
         JOIN users u ON u.id = b.user_id
         WHERE b.template_id = ? AND b.date = ? AND b.cancelled_at IS NULL
         ORDER BY b.created_at",
    )
    .bind(template_id)
    .bind(&date)
    .fetch_all(&state.pool)
    .await
    .map_err(internal_error)?;

    Ok(Json(
        rows.into_iter()
            .map(|r| ParticipantResponse {
                booking_id: r.booking_id,
                user_name: r.user_name,
                user_email: r.user_email,
            })
            .collect(),
    ))
}

async fn create_booking(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<BookingRequest>,
) -> Result<(StatusCode, Json<BookingResponse>), (StatusCode, Json<serde_json::Value>)> {
    // Determine who the booking is for. Precedence:
    //   1. explicit body.user_id
    //   2. cards.user_id when body.card_id is present (card-centric staff flow)
    //   3. fall back to the caller (customer self-booking)
    let booking_user_id = if let Some(target_id) = body.user_id {
        if target_id != claims.sub && !claims.role.can_book_for_others() {
            return Err((
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error": "Only staff can book for other users"})),
            ));
        }
        target_id
    } else if let Some(card_id) = body.card_id {
        let uid: Option<i64> = sqlx::query_scalar("SELECT user_id FROM cards WHERE id = ?")
            .bind(card_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(internal_error)?
            .flatten();
        let Some(uid) = uid else {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Card has no linked user"})),
            ));
        };
        if uid != claims.sub && !claims.role.can_book_for_others() {
            return Err((
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error": "Only staff can book for other users"})),
            ));
        }
        uid
    } else {
        claims.sub
    };

    // Check if class is cancelled.
    let cancelled = db::is_occurrence_cancelled(&state.pool, body.template_id, &body.date)
        .await
        .map_err(internal_error)?;

    if cancelled {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "Class is cancelled"})),
        ));
    }

    let created_by = if body.user_id.is_some() && body.user_id != Some(claims.sub) {
        Some(claims.sub)
    } else {
        None
    };

    let card_id: Option<i64> = if let Some(c) = body.card_id {
        Some(c)
    } else {
        sqlx::query_scalar::<_, i64>("SELECT id FROM cards WHERE user_id = ? LIMIT 1")
            .bind(booking_user_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(internal_error)?
    };

    let booking_id = db::create_booking(
        &state.pool,
        body.template_id,
        &body.date,
        booking_user_id,
        card_id,
        created_by,
        "manual",
    )
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("full") {
            (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": msg})),
            )
        } else {
            internal_error(e)
        }
    })?;

    // Broadcast booking update.
    let booked = db::get_booking_count(&state.pool, body.template_id, &body.date)
        .await
        .unwrap_or(0);
    let capacity: i64 = sqlx::query_scalar("SELECT capacity FROM class_templates WHERE id = ?")
        .bind(body.template_id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let _ = state.event_tx.send(ServerMsg::BookingUpdate {
        template_id: body.template_id,
        date: body.date.clone(),
        booked: booked as i32,
        capacity: capacity as i32,
    });

    Ok((
        StatusCode::CREATED,
        Json(BookingResponse {
            id: booking_id,
            template_id: body.template_id,
            date: body.date,
            user_id: booking_user_id,
        }),
    ))
}

async fn cancel_booking(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(booking_id): Path<i64>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    // Get the booking to check ownership.
    let booking = sqlx::query_as::<_, db::BookingRow>(
        "SELECT * FROM bookings WHERE id = ? AND cancelled_at IS NULL",
    )
    .bind(booking_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?
    .ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Booking not found"})),
        )
    })?;

    // Check permission: own booking or staff.
    if booking.user_id != claims.sub && !claims.role.can_cancel_any_booking() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Cannot cancel another user's booking"})),
        ));
    }

    db::cancel_booking(&state.pool, booking_id)
        .await
        .map_err(internal_error)?;

    // Broadcast booking update.
    let booked = db::get_booking_count(&state.pool, booking.template_id, &booking.date)
        .await
        .unwrap_or(0);
    let capacity: i64 = sqlx::query_scalar("SELECT capacity FROM class_templates WHERE id = ?")
        .bind(booking.template_id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let _ = state.event_tx.send(ServerMsg::BookingUpdate {
        template_id: booking.template_id,
        date: booking.date,
        booked: booked as i32,
        capacity: capacity as i32,
    });

    Ok(StatusCode::NO_CONTENT)
}

async fn my_bookings(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<Vec<BookingResponse>>, (StatusCode, Json<serde_json::Value>)> {
    let bookings = db::list_user_bookings(&state.pool, claims.sub)
        .await
        .map_err(internal_error)?;

    let responses: Vec<BookingResponse> = bookings
        .into_iter()
        .map(|b| BookingResponse {
            id: b.id,
            template_id: b.template_id,
            date: b.date,
            user_id: b.user_id,
        })
        .collect();

    Ok(Json(responses))
}
