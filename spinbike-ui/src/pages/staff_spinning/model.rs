//! Pure model of the staff spinning screen (#385) — no DOM, no signals, so
//! every rule the page renders is unit-tested under `wasm-pack test --node`.
//!
//! Date arithmetic here is plain calendar arithmetic on an injected `today`
//! (the page passes `relative_date::today_local()`, anchored to
//! Europe/Bratislava). Whether an occurrence already started, and whether a
//! booking was charged, always comes from the SERVER's flags — the client
//! never decides a time-of-day rule itself (`spinning-booking.md`).

use chrono::{Datelike, Duration, NaiveDate};
use serde::Deserialize;

use crate::pages::schedule::ClassSlot;

/// Planning horizon in weeks — the owner's decision (#385): staff see the
/// same 8 weeks the server materialises recurring bookings for.
pub const WEEKS: usize = 8;

/// One booked customer of a class occurrence, as returned by
/// `GET /api/classes/{template_id}/{date}/participants` (staff only).
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Participant {
    pub booking_id: i64,
    pub user_id: i64,
    pub user_name: String,
    /// `"manual"` (jednorazovo) | `"persistent"` (natrvalo).
    #[serde(default)]
    pub source: String,
    /// A charged, non-voided, non-zero booking — a staff cancel refunds it.
    #[serde(default)]
    pub refundable: bool,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub card_code: Option<String>,
    #[serde(default)]
    pub credit: f64,
    #[serde(default)]
    pub charged: bool,
    #[serde(default)]
    pub charged_amount: Option<f64>,
}

impl Participant {
    pub fn is_persistent(&self) -> bool {
        self.source == "persistent"
    }
}

/// Monday of the week containing `day`.
pub fn week_monday(day: NaiveDate) -> NaiveDate {
    day - Duration::days(day.weekday().num_days_from_monday() as i64)
}

/// The [`WEEKS`] Mondays of the grid, starting with the current week.
pub fn grid_mondays(today: NaiveDate) -> Vec<NaiveDate> {
    let first = week_monday(today);
    (0..WEEKS as i64)
        .map(|w| first + Duration::days(7 * w))
        .collect()
}

/// The ONE `/api/classes` range the grid fetches: Monday of this week ..
/// Sunday of the last grid week (56 days inclusive, under the server's
/// 62-day cap).
pub fn grid_range(today: NaiveDate) -> (String, String) {
    let from = week_monday(today);
    let to = from + Duration::days(7 * WEEKS as i64 - 1);
    (from.to_string(), to.to_string())
}

/// Row label of a grid week, e.g. `20.-26.10.`, or `27.10.-2.11.` when the
/// week spans two months.
pub fn week_label(monday: NaiveDate) -> String {
    let sunday = monday + Duration::days(6);
    if monday.month() == sunday.month() {
        format!("{}.-{}.{}.", monday.day(), sunday.day(), sunday.month())
    } else {
        format!(
            "{}.{}.-{}.{}.",
            monday.day(),
            monday.month(),
            sunday.day(),
            sunday.month()
        )
    }
}

/// One grid column = one weekly class template.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub template_id: i64,
    /// 0 = Monday .. 6 = Sunday.
    pub weekday: i64,
    pub start_time: String,
    pub instructor_name: Option<String>,
}

/// The grid's columns: every template appearing in the fetched occurrences,
/// ordered by weekday, then start time (then id, for a stable order).
pub fn grid_columns(slots: &[ClassSlot]) -> Vec<Column> {
    let mut cols: Vec<Column> = Vec::new();
    for s in slots {
        if cols.iter().any(|c| c.template_id == s.template_id) {
            continue;
        }
        cols.push(Column {
            template_id: s.template_id,
            weekday: s.weekday,
            start_time: s.start_time.clone(),
            instructor_name: s.instructor_name.clone(),
        });
    }
    cols.sort_by(|a, b| {
        (a.weekday, &a.start_time, a.template_id).cmp(&(b.weekday, &b.start_time, b.template_id))
    });
    cols
}

/// The occurrence date of `column` in the week starting `monday`.
pub fn cell_date(monday: NaiveDate, column: &Column) -> String {
    (monday + Duration::days(column.weekday)).to_string()
}

/// The fetched occurrence for one grid cell, if the template ran that day.
pub fn find_slot<'a>(
    slots: &'a [ClassSlot],
    template_id: i64,
    date: &str,
) -> Option<&'a ClassSlot> {
    slots
        .iter()
        .find(|s| s.template_id == template_id && s.date == date)
}

/// How full an occurrence is — drives the cell's background tint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillLevel {
    /// Under half full.
    Low,
    /// Half full or more, not full.
    Mid,
    /// No free bike left.
    Full,
}

pub fn fill_level(booked: i64, capacity: i64) -> FillLevel {
    if capacity <= 0 || booked >= capacity {
        FillLevel::Full
    } else if booked * 2 >= capacity {
        FillLevel::Mid
    } else {
        FillLevel::Low
    }
}

impl FillLevel {
    pub fn css(self) -> &'static str {
        match self {
            FillLevel::Low => "occ-cell--low",
            FillLevel::Mid => "occ-cell--mid",
            FillLevel::Full => "occ-cell--full",
        }
    }
}

/// The "Stav" column of a participant row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChargeState {
    /// The 09:00 binding charge has not run yet.
    Waiting,
    /// Charged a real amount (a staff cancel refunds it).
    Charged,
    /// Charged 0 EUR — a pass holder's free visit.
    Free,
    /// Charged, but the charge row was voided at the desk afterwards — no
    /// money is held for this booking any more.
    Voided,
}

pub fn charge_state(charged: bool, charged_amount: Option<f64>) -> ChargeState {
    if !charged {
        return ChargeState::Waiting;
    }
    match charged_amount {
        // Money is rounded to cents server-side; anything under half a cent
        // is a zero-amount (free) visit.
        Some(a) if a >= 0.005 => ChargeState::Charged,
        Some(_) => ChargeState::Free,
        // The server sends no amount for a charged booking only when its
        // charge row was voided (or, defensively, is missing — every charge
        // path today links one: charger.rs, booking_cancel.rs).
        None => ChargeState::Voided,
    }
}

impl ChargeState {
    pub fn i18n_key(self) -> &'static str {
        match self {
            ChargeState::Waiting => "spin_state_waiting",
            ChargeState::Charged => "spin_state_charged",
            ChargeState::Free => "spin_state_free",
            ChargeState::Voided => "spin_state_voided",
        }
    }
}

/// Sort key "by surname": the last word of the full name, then the whole
/// name, both case-insensitive (`users.name` is one "First Last" field).
pub fn surname_key(name: &str) -> (String, String) {
    let full = name.trim().to_lowercase();
    let last = full
        .split_whitespace()
        .next_back()
        .unwrap_or("")
        .to_string();
    (last, full)
}

/// Participants sorted by surname (stable on ties by booking id).
pub fn sorted_by_surname(mut list: Vec<Participant>) -> Vec<Participant> {
    list.sort_by(|a, b| {
        surname_key(&a.user_name)
            .cmp(&surname_key(&b.user_name))
            .then(a.booking_id.cmp(&b.booking_id))
    });
    list
}

/// Where the participant's name links: the desk's customer detail. An exact
/// card-code jump when the customer has one, else a name search.
pub fn customer_href(p: &Participant) -> String {
    use crate::pages::dashboard::helpers::urlencoding_light;
    match p
        .card_code
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        Some(code) => format!("/staff?card={}", urlencoding_light(code)),
        None => format!("/staff?q={}", urlencoding_light(p.user_name.trim())),
    }
}

/// The class preselected by the URL query (`?t=<template_id>&d=<YYYY-MM-DD>`,
/// order-free), if both parts are present and well-formed.
pub fn parse_preselect(search: &str) -> Option<(i64, String)> {
    let mut tid = None;
    let mut date = None;
    for kv in search.trim_start_matches('?').split('&') {
        if let Some(v) = kv.strip_prefix("t=") {
            tid = v.parse::<i64>().ok();
        } else if let Some(v) = kv.strip_prefix("d=") {
            date = NaiveDate::parse_from_str(v, "%Y-%m-%d")
                .ok()
                .map(|d| d.to_string());
        }
    }
    Some((tid?, date?))
}

/// Link that opens the staff spinning screen on one class occurrence.
pub fn occurrence_href(template_id: i64, date: &str) -> String {
    format!("/staff/spinning?t={template_id}&d={date}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    // No wasm_bindgen_test_configure! — CI uses wasm-pack test --node (not browser).

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn slot(template_id: i64, date: &str, weekday: i64, start: &str) -> ClassSlot {
        ClassSlot {
            template_id,
            date: date.to_string(),
            weekday,
            start_time: start.to_string(),
            duration_minutes: 50,
            instructor_id: None,
            instructor_name: Some(format!("I{template_id}")),
            capacity: 19,
            booked: 0,
            cancelled: false,
            user_booked: false,
            user_booking_id: None,
            user_booking_source: None,
            started: false,
            cancel_open: true,
        }
    }

    fn person(id: i64, name: &str) -> Participant {
        Participant {
            booking_id: id,
            user_id: id,
            user_name: name.to_string(),
            source: "manual".to_string(),
            refundable: false,
            phone: None,
            card_code: None,
            credit: 0.0,
            charged: false,
            charged_amount: None,
        }
    }

    #[wasm_bindgen_test]
    fn grid_starts_on_this_weeks_monday_and_spans_eight_weeks() {
        // Wednesday 2026-10-21 -> Monday 2026-10-19.
        let mondays = grid_mondays(d(2026, 10, 21));
        assert_eq!(mondays.len(), 8);
        assert_eq!(mondays[0], d(2026, 10, 19));
        assert_eq!(mondays[7], d(2026, 12, 7));
        // A Monday is its own week start; a Sunday belongs to the week before.
        assert_eq!(week_monday(d(2026, 10, 19)), d(2026, 10, 19));
        assert_eq!(week_monday(d(2026, 10, 25)), d(2026, 10, 19));
    }

    #[wasm_bindgen_test]
    fn grid_range_is_56_days_monday_to_sunday() {
        let (from, to) = grid_range(d(2026, 10, 25));
        assert_eq!(from, "2026-10-19");
        assert_eq!(to, "2026-12-13");
        let span = d(2026, 12, 13) - d(2026, 10, 19);
        assert_eq!(span.num_days() + 1, 56);
    }

    #[wasm_bindgen_test]
    fn week_label_same_and_cross_month() {
        assert_eq!(week_label(d(2026, 10, 19)), "19.-25.10.");
        assert_eq!(week_label(d(2026, 10, 26)), "26.10.-1.11.");
    }

    #[wasm_bindgen_test]
    fn columns_are_unique_templates_by_weekday_then_time() {
        let slots = vec![
            slot(7, "2026-10-21", 2, "18:30"),
            slot(3, "2026-10-19", 0, "18:00"),
            slot(9, "2026-10-21", 2, "18:00"),
            slot(3, "2026-10-26", 0, "18:00"),
        ];
        let cols = grid_columns(&slots);
        let ids: Vec<i64> = cols.iter().map(|c| c.template_id).collect();
        assert_eq!(ids, vec![3, 9, 7]);
        assert_eq!(cols[0].instructor_name.as_deref(), Some("I3"));
        assert_eq!(cell_date(d(2026, 10, 26), &cols[1]), "2026-10-28");
        assert!(find_slot(&slots, 3, "2026-10-26").is_some());
        assert!(find_slot(&slots, 3, "2026-10-27").is_none());
        assert!(find_slot(&slots, 9, "2026-10-19").is_none());
    }

    #[wasm_bindgen_test]
    fn fill_level_boundaries() {
        assert_eq!(fill_level(0, 19), FillLevel::Low);
        assert_eq!(fill_level(9, 19), FillLevel::Low);
        assert_eq!(fill_level(10, 19), FillLevel::Mid);
        assert_eq!(fill_level(10, 20), FillLevel::Mid);
        assert_eq!(fill_level(18, 19), FillLevel::Mid);
        assert_eq!(fill_level(19, 19), FillLevel::Full);
        assert_eq!(fill_level(20, 19), FillLevel::Full);
        assert_eq!(fill_level(0, 0), FillLevel::Full);
        assert_eq!(FillLevel::Full.css(), "occ-cell--full");
    }

    #[wasm_bindgen_test]
    fn charge_state_from_server_flags() {
        assert_eq!(charge_state(false, None), ChargeState::Waiting);
        assert_eq!(charge_state(true, Some(5.0)), ChargeState::Charged);
        assert_eq!(charge_state(true, Some(0.01)), ChargeState::Charged);
        assert_eq!(charge_state(true, Some(0.0)), ChargeState::Free);
        assert_eq!(charge_state(true, None), ChargeState::Voided);
        assert_eq!(ChargeState::Voided.i18n_key(), "spin_state_voided");
        assert_eq!(ChargeState::Waiting.i18n_key(), "spin_state_waiting");
    }

    #[wasm_bindgen_test]
    fn participants_sort_by_surname_case_insensitively() {
        let sorted = sorted_by_surname(vec![
            person(1, "Jana Zelena"),
            person(2, "peter adamec"),
            person(3, "Anna Novakova"),
            person(4, "Cher"),
        ]);
        let names: Vec<&str> = sorted.iter().map(|p| p.user_name.as_str()).collect();
        assert_eq!(
            names,
            vec!["peter adamec", "Cher", "Anna Novakova", "Jana Zelena"]
        );
    }

    #[wasm_bindgen_test]
    fn preselect_query_round_trips_and_rejects_garbage() {
        assert_eq!(
            parse_preselect("?t=4&d=2026-10-21"),
            Some((4, "2026-10-21".to_string()))
        );
        assert_eq!(
            parse_preselect("?d=2026-10-21&x=1&t=4"),
            Some((4, "2026-10-21".to_string()))
        );
        let href = occurrence_href(4, "2026-10-21");
        assert_eq!(
            parse_preselect(href.split_once('?').unwrap().1),
            Some((4, "2026-10-21".to_string()))
        );
        assert_eq!(parse_preselect(""), None);
        assert_eq!(parse_preselect("?t=4"), None);
        assert_eq!(parse_preselect("?t=x&d=2026-10-21"), None);
        assert_eq!(parse_preselect("?t=4&d=2026-13-01"), None);
    }

    #[wasm_bindgen_test]
    fn customer_link_prefers_card_code() {
        let mut p = person(1, "Jana Zelena");
        assert_eq!(customer_href(&p), "/staff?q=Jana%20Zelena");
        p.card_code = Some(" SB-12 ".to_string());
        assert_eq!(customer_href(&p), "/staff?card=SB-12");
        p.card_code = Some("  ".to_string());
        assert_eq!(customer_href(&p), "/staff?q=Jana%20Zelena");
    }
}
