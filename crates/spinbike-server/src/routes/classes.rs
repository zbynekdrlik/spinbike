use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post, put},
};
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::auth::{AuthUser, OptionalAuthUser, StaffUser, require_live_session};
use crate::booking_rules;
use crate::db::booking_cancel::{self, CancelKind, CancelResult};
use crate::db::booking_guests::{self, SetGuestsResult};
use crate::db::classes as db;
use crate::error::ApiError;
use crate::routes::internal_error;
use spinbike_core::errors::ErrorCode;
use spinbike_core::ws::ServerMsg;

/// The longest `GET /api/classes` range accepted, in calendar days
/// INCLUSIVE of both ends (#385). The staff 8-week occupancy grid asks for
/// 56 (Monday .. Sunday seven weeks later); anything above this cap is a 400
/// `range_too_long` instead of a per-day loop over an arbitrary span.
pub const MAX_RANGE_DAYS: i64 = 62;

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
    /// The instructor's display name (#381) — the schedule used to render
    /// only `instructor_id` ("Instruktor #2"). `None` when the template has
    /// no instructor.
    pub instructor_name: Option<String>,
    pub capacity: i64,
    pub booked: i64,
    pub cancelled: bool,
    pub user_booked: bool,
    pub user_booking_id: Option<i64>,
    pub user_booking_source: Option<String>,
    /// #389: guests on the caller's own booking (0 when none / not booked) —
    /// the customer card's "+ host" stepper starts from it.
    pub user_booking_guests: i64,
    /// #382 R4: the occurrence already started (gym-local now) — it can no
    /// longer be booked. Computed server-side so the client never does its
    /// own time-zone math.
    pub started: bool,
    /// #382 R2: a customer could still cancel a booking on this occurrence
    /// right now (before 09:00 on the class day).
    pub cancel_open: bool,
}

#[derive(Deserialize)]
pub struct BookingRequest {
    pub template_id: i64,
    pub date: String,
    pub user_id: Option<i64>,
}

#[derive(Serialize)]
pub struct BookingResponse {
    pub id: i64,
    pub template_id: i64,
    pub date: String,
    pub user_id: i64,
}

/// A user's own booking, enriched with its class start time + instructor
/// (#146) — the `/api/my/bookings` response shape. Kept separate from
/// `BookingResponse` (the `create_booking` echo, which the frontend only
/// reads `id` from) rather than bolting always-null fields onto a shared
/// type for one caller — same reasoning as the `_coded` API variant
/// pattern (see `.claude/skills/frontend-pwa/SKILL.md`).
#[derive(Serialize)]
pub struct MyBookingResponse {
    pub id: i64,
    pub template_id: i64,
    pub date: String,
    pub user_id: i64,
    /// Class start time ("HH:MM"), joined from `class_templates`.
    pub start_time: String,
    pub instructor_name: Option<String>,
    /// `"manual"` | `"persistent"` — the recurring mark (#382).
    pub source: String,
    /// #382 R2: whether the customer may still cancel this booking now
    /// (before 09:00 on the class day and not yet charged).
    pub cancel_open: bool,
    /// #389: guests the customer brings (each paid from their credit).
    pub guests: i64,
}

/// A participant in a class (booking joined with user info).
#[derive(Serialize)]
pub struct ParticipantResponse {
    pub booking_id: i64,
    /// The booked customer's id — lets the staff walk-in picker mark
    /// customers already in the class instead of offering a booking that
    /// would 409 `already_booked` (#378).
    pub user_id: i64,
    pub user_name: String,
    pub user_email: String,
    /// `"manual"` | `"persistent"` — staff marks recurring bookings
    /// ("pravidelne", #382 R8).
    pub source: String,
    /// The booking carries a non-zero, non-voided charge — a staff cancel
    /// refunds it (#382 R3/R8).
    pub refundable: bool,
    /// #385 staff table: the customer's phone, so the desk can call a
    /// regular who never installed the app.
    pub phone: Option<String>,
    /// #385: the customer's card code — the staff table links the name to
    /// the desk's customer detail (`/staff?card=<code>`).
    pub card_code: Option<String>,
    /// #385: the customer's CURRENT credit (negative = owes money).
    pub credit: f64,
    /// #385: the 09:00 binding charge already ran for this booking
    /// (`bookings.charged_at` set) — "strhnute"/"zadarmo" vs "caka na 9:00".
    pub charged: bool,
    /// #385: the amount the linked, NON-voided charge row took (positive;
    /// `0.0` for a pass holder's free visit). `None` while uncharged, or
    /// when the charge row was voided.
    pub charged_amount: Option<f64>,
    /// #389: guests on this booking (each takes a seat, paid by the host).
    pub guests: i64,
    /// #389: what the host's still-held, non-voided GUEST charges took
    /// (positive; 0.0 while uncharged) — a staff cancel refunds it too.
    pub guest_charged_amount: f64,
}

/// `PUT /api/bookings/{id}/guests` body (#389).
#[derive(Deserialize)]
pub struct SetGuestsRequest {
    pub guests: i64,
}

/// `PUT /api/bookings/{id}/guests` response (#389).
#[derive(Serialize)]
pub struct SetGuestsResponse {
    pub booking_id: i64,
    pub guests: i64,
    /// Credit debited now (staff added guests after the 09:00 charge).
    pub charged: f64,
    /// Credit refunded now (staff removed guests after the 09:00 charge).
    pub refunded: f64,
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
        .route("/api/bookings/{id}/guests", put(set_guests))
        .route("/api/my/bookings", get(my_bookings))
}

async fn list_classes(
    State(state): State<AppState>,
    OptionalAuthUser(claims): OptionalAuthUser,
    Query(query): Query<ScheduleQuery>,
) -> Result<Json<Vec<ClassOccurrenceResponse>>, ApiError> {
    let from = NaiveDate::parse_from_str(&query.from, "%Y-%m-%d")
        .map_err(|_| super::bad_request("Invalid 'from' date format, expected YYYY-MM-DD"))?;
    let to = NaiveDate::parse_from_str(&query.to, "%Y-%m-%d")
        .map_err(|_| super::bad_request("Invalid 'to' date format, expected YYYY-MM-DD"))?;
    let days_inclusive = (to - from).num_days() + 1;
    if days_inclusive > MAX_RANGE_DAYS {
        tracing::info!(
            from = %from,
            to = %to,
            days_inclusive,
            max = MAX_RANGE_DAYS,
            "class list rejected: range too long"
        );
        return Err(ApiError::BadRequestCode(ErrorCode::RangeTooLong));
    }

    let templates = db::list_active_templates(&state.pool)
        .await
        .map_err(internal_error)?;
    let instructor_names = crate::db::instructors::names_by_id(&state.pool)
        .await
        .map_err(internal_error)?;

    // Gym-local wall clock for the started / cancel-open flags (#382) —
    // never `chrono::Local` (bratislava-tz.md).
    let now = crate::util::now_bratislava();
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
            let (user_booked, user_booking_id, user_booking_source, user_booking_guests) =
                if let Some(ref c) = claims {
                    let bookings = db::list_bookings_for_class(&state.pool, tmpl.id, &date_str)
                        .await
                        .unwrap_or_default();
                    let user_booking = bookings.iter().find(|b| b.user_id == c.sub);
                    (
                        user_booking.is_some(),
                        user_booking.map(|b| b.id),
                        user_booking.map(|b| b.source.clone()),
                        user_booking.map(|b| b.guests).unwrap_or(0),
                    )
                } else {
                    (false, None, None, 0)
                };

            occurrences.push(ClassOccurrenceResponse {
                template_id: tmpl.id,
                date: date_str,
                weekday: tmpl.weekday,
                start_time: tmpl.start_time.clone(),
                duration_minutes: tmpl.duration_minutes,
                instructor_id: tmpl.instructor_id,
                instructor_name: tmpl
                    .instructor_id
                    .and_then(|iid| instructor_names.get(&iid).cloned()),
                capacity: tmpl.capacity,
                booked,
                cancelled,
                user_booked,
                user_booking_id,
                user_booking_source,
                user_booking_guests,
                started: booking_rules::has_started(current, &tmpl.start_time, now),
                cancel_open: !booking_rules::is_charge_due(current, now),
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
    _: StaffUser,
    Path((template_id, date)): Path<(i64, String)>,
) -> Result<Json<Vec<ParticipantResponse>>, ApiError> {
    #[derive(sqlx::FromRow)]
    struct Row {
        booking_id: i64,
        user_id: i64,
        user_name: String,
        user_email: String,
        source: String,
        refundable: i64,
        phone: Option<String>,
        card_code: Option<String>,
        credit: f64,
        charged: i64,
        charged_amount: Option<f64>,
        guests: i64,
        guest_charged_amount: f64,
    }

    // `refundable` mirrors `db::booking_cancel::cancel_booking`'s refund condition:
    // charged, and the linked charge row is a non-voided NEGATIVE amount (a
    // pass holder's 0 EUR visit needs no refund). `charged_amount` is that
    // same linked row's amount as a positive number, NULL while uncharged or
    // once the row was voided (#385) — one SELECT, same joins.
    let rows = sqlx::query_as::<_, Row>(
        "SELECT b.id AS booking_id, b.user_id AS user_id, u.name AS user_name, COALESCE(u.email, '') AS user_email,
                b.source AS source,
                COALESCE(b.charged_at IS NOT NULL AND ct.amount < 0 AND ct.deleted_at IS NULL, 0)
                    AS refundable,
                u.phone AS phone, u.card_code AS card_code, u.credit AS credit,
                (b.charged_at IS NOT NULL) AS charged,
                CASE WHEN b.charged_at IS NOT NULL AND ct.id IS NOT NULL AND ct.deleted_at IS NULL
                     THEN ABS(ct.amount) END AS charged_amount,
                b.guests AS guests,
                COALESCE((SELECT ROUND(SUM(-gt.amount), 2)
                            FROM booking_guest_charges g
                            JOIN transactions gt ON gt.id = g.transaction_id
                           WHERE g.booking_id = b.id AND g.released_at IS NULL
                             AND gt.deleted_at IS NULL AND gt.amount < 0), 0.0)
                    AS guest_charged_amount
         FROM bookings b
         JOIN users u ON u.id = b.user_id  -- no deleted_at filter: booking history for soft-deleted users remains part of class records
         LEFT JOIN transactions ct ON ct.id = b.charge_transaction_id
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
                user_id: r.user_id,
                user_name: r.user_name,
                user_email: r.user_email,
                source: r.source,
                refundable: r.refundable != 0,
                phone: r.phone,
                card_code: r.card_code,
                credit: r.credit,
                charged: r.charged != 0,
                charged_amount: r.charged_amount,
                guests: r.guests,
                guest_charged_amount: r.guest_charged_amount,
            })
            .collect(),
    ))
}

async fn create_booking(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Json(body): Json<BookingRequest>,
) -> Result<(StatusCode, Json<BookingResponse>), ApiError> {
    // #277: a dead caller session (missing/soft-deleted/blocked) must never
    // be able to book — checked first, before any other logic, same
    // placement as door.rs's session-invalid gate. This also fixes the
    // FK-violation 500 a missing `sub` used to hit on the `bookings` INSERT
    // below (no row → 401 now, never reaches the insert at all).
    require_live_session(&state.pool, claims.sub).await?;

    // Determine who the booking is for. Precedence:
    //   1. explicit body.user_id
    //   2. fall back to the caller (customer self-booking)
    let booking_user_id = if let Some(target_id) = body.user_id {
        // Ownership-mixed guard: allowed if the caller books for THEMSELVES
        // (target == claims.sub) OR holds the staff capability. A pure role
        // extractor (StaffUser) can't express the "or owner" arm, so this one
        // stays inline and takes AuthUser (needs both the role AND claims.sub).
        if target_id != claims.sub && !claims.role.can_book_for_others() {
            return Err(ApiError::Forbidden(ErrorCode::StaffRequired));
        }
        target_id
    } else {
        claims.sub
    };

    // Check if class is cancelled.
    let cancelled = db::is_occurrence_cancelled(&state.pool, body.template_id, &body.date)
        .await
        .map_err(internal_error)?;

    if cancelled {
        return Err(ApiError::conflict(ErrorCode::ClassCancelled));
    }

    // #382 R4: no booking for an occurrence that already started — a past
    // date, or today after the start time. Applies to staff too (a walk-in
    // for a running class is recorded at the desk as a visit, not a booking).
    let date = NaiveDate::parse_from_str(&body.date, "%Y-%m-%d")
        .map_err(|_| super::bad_request("Invalid 'date' format, expected YYYY-MM-DD"))?;
    let start_time: Option<String> =
        sqlx::query_scalar("SELECT start_time FROM class_templates WHERE id = ?")
            .bind(body.template_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(internal_error)?;
    let Some(start_time) = start_time else {
        return Err(super::bad_request("Unknown class template"));
    };
    if booking_rules::has_started(date, &start_time, crate::util::now_bratislava()) {
        tracing::info!(
            template_id = body.template_id,
            date = %body.date,
            start_time = %start_time,
            user_id = booking_user_id,
            caller = claims.sub,
            "booking rejected: the class occurrence already started"
        );
        return Err(ApiError::conflict(ErrorCode::ClassAlreadyStarted));
    }

    // Stamp `created_by` whenever the booking's user_id differs from the caller.
    let created_by = if booking_user_id != claims.sub {
        Some(claims.sub)
    } else {
        None
    };

    let booking_id = db::create_booking(
        &state.pool,
        body.template_id,
        &body.date,
        booking_user_id,
        created_by,
        "manual",
    )
    .await
    .map_err(|e| {
        if matches!(e, crate::db::DbError::ClassFull) {
            ApiError::conflict_message(ErrorCode::ClassFull, e.to_string())
        } else if matches!(e, crate::db::DbError::UniqueViolation) {
            // (A FULL class reports `class_full` even for a user already in
            // it: the capacity guard in the INSERT ... SELECT runs before
            // the index can fire. The walk-in picker already disables
            // booked customers, so that path is not reachable from staff.)
            // #378: the partial unique index `idx_bookings_active
            // (template_id, date, user_id) WHERE cancelled_at IS NULL` is the
            // source of truth for "already booked" — no racy pre-check
            // SELECT. Its violation is an expected business outcome, not a 500.
            tracing::info!(
                template_id = body.template_id,
                date = %body.date,
                user_id = booking_user_id,
                "booking rejected: user already holds an active booking for this class"
            );
            ApiError::conflict(ErrorCode::AlreadyBooked)
        } else {
            internal_error(e)
        }
    })?;

    broadcast_occupancy(&state, body.template_id, &body.date).await;

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
) -> Result<StatusCode, ApiError> {
    // #277: reject a dead caller session before even looking up the
    // booking — a blocked/deleted/missing session must get 401 regardless
    // of which booking id it names.
    require_live_session(&state.pool, claims.sub).await?;

    // Get the booking to check ownership + the cancel deadline.
    let booking: Option<(i64, i64, String, Option<String>)> = sqlx::query_as(
        "SELECT user_id, template_id, date, charged_at
         FROM bookings WHERE id = ? AND cancelled_at IS NULL",
    )
    .bind(booking_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?;
    let Some((owner_id, template_id, date, charged_at)) = booking else {
        return Err(ApiError::NotFound(ErrorCode::BookingNotFound));
    };

    // Ownership-mixed guard: own booking OR staff. Needs both the role AND
    // claims.sub (the booking owner check), so it can't move to a pure role
    // extractor and stays inline on AuthUser.
    let is_staff = claims.role.can_cancel_any_booking();
    if owner_id != claims.sub && !is_staff {
        return Err(ApiError::Forbidden(ErrorCode::BookingNotOwned));
    }

    // #382 R2: a customer may cancel only before 09:00 on the class day (and
    // never a charged booking). Staff/admin may cancel at any time — a
    // charged booking is then refunded (R3, inside `cancel_booking`).
    let kind = if is_staff {
        CancelKind::Staff
    } else {
        let class_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(internal_error)?;
        let now = crate::util::now_bratislava();
        if !booking_rules::customer_may_cancel(class_date, charged_at.is_some(), now) {
            tracing::info!(
                booking_id,
                user_id = claims.sub,
                class_date = %date,
                charged = charged_at.is_some(),
                "booking cancel rejected: past the 09:00 deadline of the class day"
            );
            return Err(ApiError::conflict(ErrorCode::CancelDeadlinePassed));
        }
        CancelKind::Customer
    };

    match booking_cancel::cancel_booking(&state.pool, booking_id, Some(claims.sub), kind)
        .await
        .map_err(internal_error)?
    {
        CancelResult::Cancelled(_) => {}
        CancelResult::NotActive => return Err(ApiError::NotFound(ErrorCode::BookingNotFound)),
        // The 09:00 charger charged it between the deadline check above and
        // the cancel transaction — binding, same answer as a late cancel.
        CancelResult::AlreadyCharged => {
            return Err(ApiError::conflict(ErrorCode::CancelDeadlinePassed));
        }
    }
    broadcast_occupancy(&state, template_id, &date).await;

    Ok(StatusCode::NO_CONTENT)
}

/// Broadcast the occupancy (seats) of one class occurrence to every
/// connected schedule. Best-effort: the write already committed, so a failed
/// read is logged and the broadcast skipped (never a misleading `0/0`).
async fn broadcast_occupancy(state: &AppState, template_id: i64, date: &str) {
    let booked = match db::get_booking_count(&state.pool, template_id, date).await {
        Ok(n) => n,
        Err(e) => {
            tracing::warn!(template_id, date, error = ?e, "occupancy broadcast skipped: seat count failed");
            return;
        }
    };
    let capacity: i64 = match sqlx::query_scalar(
        "SELECT capacity FROM class_templates WHERE id = ?",
    )
    .bind(template_id)
    .fetch_one(&state.pool)
    .await
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(template_id, date, error = ?e, "occupancy broadcast skipped: capacity read failed");
            return;
        }
    };
    let _ = state.event_tx.send(ServerMsg::BookingUpdate {
        template_id,
        date: date.to_string(),
        booked: booked as i32,
        capacity: capacity as i32,
    });
}

/// #389: set how many guests the booking's host brings (and pays for).
/// Owner or staff only. A customer may change it until 09:00 on the class
/// day (the cancel deadline, `booking_rules::customer_may_cancel`); staff at
/// any time — after the charge an increase is charged and a decrease
/// refunded at once (`db::booking_guests::set_guests`).
async fn set_guests(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
    Path(booking_id): Path<i64>,
    Json(body): Json<SetGuestsRequest>,
) -> Result<Json<SetGuestsResponse>, ApiError> {
    require_live_session(&state.pool, claims.sub).await?;

    if !spinbike_core::booking::guests_in_range(body.guests) {
        tracing::info!(
            booking_id,
            caller = claims.sub,
            guests = body.guests,
            "guest change rejected: out of range"
        );
        return Err(ApiError::BadRequestCode(ErrorCode::TooManyGuests));
    }

    let booking: Option<(i64, i64, String, Option<String>)> = sqlx::query_as(
        "SELECT user_id, template_id, date, charged_at
         FROM bookings WHERE id = ? AND cancelled_at IS NULL",
    )
    .bind(booking_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_error)?;
    let Some((owner_id, template_id, date, charged_at)) = booking else {
        return Err(ApiError::NotFound(ErrorCode::BookingNotFound));
    };

    let is_staff = claims.role.can_cancel_any_booking();
    if owner_id != claims.sub && !is_staff {
        tracing::info!(
            booking_id,
            caller = claims.sub,
            owner_id,
            "guest change rejected: not the caller's booking"
        );
        return Err(ApiError::Forbidden(ErrorCode::BookingNotOwned));
    }
    if db::is_occurrence_cancelled(&state.pool, template_id, &date)
        .await
        .map_err(internal_error)?
    {
        return Err(ApiError::conflict(ErrorCode::ClassCancelled));
    }
    if !is_staff {
        let class_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(internal_error)?;
        let now = crate::util::now_bratislava();
        if !booking_rules::customer_may_cancel(class_date, charged_at.is_some(), now) {
            tracing::info!(
                booking_id,
                user_id = claims.sub,
                class_date = %date,
                charged = charged_at.is_some(),
                "guest change rejected: past the 09:00 deadline of the class day"
            );
            return Err(ApiError::conflict(ErrorCode::GuestsDeadlinePassed));
        }
    }

    let change = match booking_guests::set_guests(
        &state.pool,
        booking_id,
        body.guests,
        Some(claims.sub),
        is_staff,
    )
    .await
    .map_err(internal_error)?
    {
        SetGuestsResult::Changed(c) => c,
        SetGuestsResult::NotActive => return Err(ApiError::NotFound(ErrorCode::BookingNotFound)),
        // The 09:00 charger charged it after the deadline check above.
        SetGuestsResult::AlreadyCharged => {
            return Err(ApiError::conflict(ErrorCode::GuestsDeadlinePassed));
        }
        SetGuestsResult::ClassFull => return Err(ApiError::conflict(ErrorCode::ClassFull)),
    };

    if change.guests != change.old_guests {
        broadcast_occupancy(&state, template_id, &date).await;
    }
    Ok(Json(SetGuestsResponse {
        booking_id,
        guests: change.guests,
        charged: change.charged,
        refunded: change.refunded,
    }))
}

async fn my_bookings(
    State(state): State<AppState>,
    AuthUser(claims): AuthUser,
) -> Result<Json<Vec<MyBookingResponse>>, ApiError> {
    // #277: a dead session must never see its own (still-listed) bookings —
    // reject before the read.
    require_live_session(&state.pool, claims.sub).await?;

    let bookings = db::list_user_bookings(&state.pool, claims.sub)
        .await
        .map_err(internal_error)?;

    let now = crate::util::now_bratislava();
    let responses: Vec<MyBookingResponse> = bookings
        .into_iter()
        .map(|b| {
            let cancel_open = NaiveDate::parse_from_str(&b.date, "%Y-%m-%d")
                .map(|d| booking_rules::customer_may_cancel(d, b.charged_at.is_some(), now))
                .unwrap_or(false);
            MyBookingResponse {
                id: b.id,
                template_id: b.template_id,
                date: b.date,
                user_id: b.user_id,
                start_time: b.start_time,
                instructor_name: b.instructor_name,
                source: b.source,
                cancel_open,
                guests: b.guests,
            }
        })
        .collect();

    Ok(Json(responses))
}
