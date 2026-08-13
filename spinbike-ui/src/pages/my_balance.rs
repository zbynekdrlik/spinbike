//! Customer self-service dashboard at `/my/balance` — credit, monthly-pass
//! status, hold-to-open door button (#92), recent visits.
//!
//! The DoorButton state machine lives in `components::door_button`; this
//! page just renders the button alongside credit / pass / recent-visits.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use spinbike_core::reports::{EventKind, classify};

use crate::api;
use crate::components::{DoorButton, InstallPrompt, PushToggle, PushToggleSurface};
use crate::dates;
use crate::i18n::{self, Lang, fmt_date_short, tf};

#[derive(Debug, Clone, serde::Deserialize)]
struct BalanceResp {
    #[allow(dead_code)]
    user_id: i64,
    // #319: the personalized "Ahoj, {name}" greeting that used to read this
    // field was removed — the shortened name now lives in the header
    // (Navbar) instead. Kept here (not deleted) to preserve the honest
    // wire shape of GET /api/my/balance.
    #[allow(dead_code)]
    name: String,
    credit: f64,
    #[allow(dead_code)]
    card_code: Option<String>,
    allow_self_entry: bool,
    monthly_pass_active_until: Option<String>,
    recent: Vec<RecentTx>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct RecentTx {
    #[allow(dead_code)]
    id: i64,
    created_at: String,
    action: String,
    amount: f64,
    valid_until: Option<String>,
    note: Option<String>,
    /// #328: the trustworthy door-press classifier — see the matching
    /// server-side doc comment on `RecentTx` (`routes/my_balance.rs`).
    is_door_press: bool,
    service_name_sk: Option<String>,
    service_name_en: Option<String>,
    /// #357: who recorded this row at the desk. `None` for a movement the
    /// customer caused themselves — see the server-side doc comment on
    /// `RecentTx` (`routes/my_balance.rs`).
    staff_name: Option<String>,
}

impl RecentTx {
    /// Delegates to the shared `i18n::service_label` (#147) — same helper
    /// the admin `TxnInfo::service_label` (dashboard/mod.rs) uses.
    fn service_label(&self, lang: Lang) -> Option<&str> {
        i18n::service_label(&self.service_name_sk, &self.service_name_en, lang)
    }
}

#[component]
pub fn MyBalancePage() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (data, set_data) = signal(None::<BalanceResp>);
    let (loading, set_loading) = signal(true);
    let (error, set_error) = signal(None::<api::CodedError>);

    let load = move || {
        set_loading.set(true);
        spawn_local(async move {
            // get_coded (#145): carries the server's `error_code` so the
            // banner below can localize it instead of showing raw English.
            match api::get_coded::<BalanceResp>("/api/my/balance").await {
                Ok(d) => {
                    set_data.set(Some(d));
                    set_error.set(None);
                }
                Err(e) => set_error.set(Some(e)),
            }
            set_loading.set(false);
        });
    };
    Effect::new(move |_| load());

    let allowed_signal: Signal<bool> = Signal::derive(move || {
        data.with(|d| d.as_ref().map(|d| d.allow_self_entry).unwrap_or(false))
    });

    let on_door_success = Callback::new(move |()| {
        // Refresh balance — credit / pass / recent rows may have changed.
        spawn_local(async move {
            if let Ok(d) = api::get::<BalanceResp>("/api/my/balance").await {
                set_data.set(Some(d));
            }
        });
    });

    view! {
        // #319: the standalone personalized "Ahoj, {name}" greeting was
        // removed — the customer's (shortened) name now lives in the
        // header (Navbar) instead, so this title is the same static,
        // non-personalized one every other page already uses.
        <h1 class="page-title">{move || i18n::t(lang.get(), "my_balance")}</h1>

        // Credit + pass cards — re-render reactively on data changes (no
        // remount of children; just text updates).
        <div class="card-credit" data-testid="my-balance-credit">
            <div class="card-credit__label">{move || i18n::t(lang.get(), "my_balance_credit")}</div>
            <div class="card-credit__value">
                "\u{20ac} "
                {move || data.with(|d| d.as_ref().map(|d| format!("{:.2}", d.credit)).unwrap_or_else(|| "—".into()))}
            </div>
        </div>

        <div class="card-pass" data-testid="my-balance-pass">
            <div class="card-pass__label">{move || i18n::t(lang.get(), "service_kind_monthly_pass")}</div>
            <div class="card-pass__value">
                {move || data.with(|d| {
                    let Some(b) = d.as_ref() else {
                        return String::new();
                    };
                    match &b.monthly_pass_active_until {
                        Some(ts) => match dates::parse_server_date(ts) {
                            Some(d) => tf(lang.get(), "monthly_pass_active_until", &[&fmt_date_short(d, lang.get())]),
                            None => tf(lang.get(), "monthly_pass_active_until", &[ts]),
                        },
                        None => i18n::t(lang.get(), "monthly_pass_not_active").to_string(),
                    }
                })}
            </div>
        </div>

        // DoorButton rendered ONCE at the top level. It reads `allowed`
        // reactively but its component instance is stable — `on_door_success`
        // refreshing the parent's `data` signal does NOT remount the button,
        // so the Success banner stays on screen until the auto-reset timer.
        <DoorButton allowed=allowed_signal on_success=on_door_success />

        // Install-to-home-screen nudge (#110) — renders nothing once
        // installed or on a browser offering neither install path.
        <InstallPrompt />

        // Notification settings row + one-time permission prompt (#264,
        // redesigned #303) — renders nothing while loading, unsupported, or
        // server-disabled. Auto-subscribes silently when permission is
        // already granted; otherwise shows a one-time proactive prompt.
        // #316: the main screen only shows it while actionable (Off,
        // Blocked) — once notifications are already on, the full row lives
        // on /my/settings instead.
        <PushToggle surface=PushToggleSurface::MainBalance />

        // Loading spinner / error banner / recent visits — these update
        // reactively on data changes.
        {move || {
            if let Some(e) = error.get() {
                let msg = i18n::localize_api_error(lang.get(), e.code, &e.message);
                return view! { <div class="alert alert-error">{msg}</div> }.into_any();
            }
            if loading.get() && data.with(|d| d.is_none()) {
                return view! { <div class="text-center mt-3"><span class="spinner"></span></div> }.into_any();
            }
            data.with(|d| match d {
                None => view! { <div class="empty-state">{i18n::t(lang.get(), "unable_to_load")}</div> }.into_any(),
                Some(b) => {
                    let recent_rows = b.recent.clone();
                    let lang_now = lang.get();
                    view! {
                        <h2 class="recent-visits__heading">{i18n::t(lang_now, "my_balance_recent_movements")}</h2>
                        <ul class="recent-visits">
                            {recent_rows.into_iter().map(|t| {
                                let date_label = format_tx_date_label(&t.created_at, lang_now);

                                // Derive the movement kind from the SAME shared
                                // classifier the admin uses, so the customer sees
                                // the SAME Slovak labels instead of the raw DB token.
                                let valid_until = t.valid_until.as_deref().and_then(dates::parse_server_date);
                                let kind = classify(&t.action, t.amount, valid_until);
                                let action_label = i18n::t(lang_now, i18n::tx_label_key(kind)).to_string();

                                // Pass-sale rows show the expiry date, like the admin row.
                                let until_suffix = if matches!(kind, EventKind::PassSale) {
                                    valid_until
                                        .map(|d| format!(" \u{b7} {} {}", i18n::t(lang_now, "tx_until_short"), fmt_date_short(d, lang_now)))
                                        .unwrap_or_default()
                                } else {
                                    String::new()
                                };

                                // Signed + coloured amount (matches admin `{:+.2}`),
                                // so a top-up and a spend are distinguishable. €0 rows
                                // (visits) show no amount — the label carries the meaning.
                                let amount_label = if t.amount.abs() < 0.005 {
                                    String::new()
                                } else {
                                    format!("{:+.2}", t.amount)
                                };
                                let amount_class = if t.amount >= 0.0 {
                                    "list-row__amount list-row__amount--pos"
                                } else {
                                    "list-row__amount list-row__amount--neg"
                                };

                                // Service name (#147) — same joined data + language
                                // pick the admin transactions list uses. Shows nothing
                                // when the movement wasn't tied to a service (e.g. a
                                // plain top-up), matching the graceful-fallback style
                                // already used for door notes below.
                                let service_suffix = t.service_label(lang_now)
                                    .map(|s| format!(" \u{b7} {s}"))
                                    .unwrap_or_default();

                                let sub_note = recent_tx_sub_note(t.note.as_deref(), t.is_door_press, lang_now);
                                let source = recent_tx_source(t.is_door_press, t.staff_name.as_deref(), lang_now);

                                view! {
                                    <li data-testid="recent-visit" class="list-row">
                                        <div class="list-row__main">
                                            <div class="list-row__title">{action_label}{until_suffix}</div>
                                            <div class="list-row__sub">{date_label}{service_suffix}{sub_note}{source}</div>
                                        </div>
                                        <div class=amount_class>{amount_label}</div>
                                    </li>
                                }
                            }).collect_view()}
                        </ul>
                    }.into_any()
                }
            })
        }}
    }
}

/// Render a recent-transaction row's display date from its raw server
/// `created_at` (a UTC instant, the same field `last_visit_at` is derived
/// from elsewhere). Extracted for testability — see #242.
fn format_tx_date_label(created_at: &str, lang: Lang) -> String {
    dates::parse_server_date_local(created_at)
        .map(|d| fmt_date_short(d, lang))
        .unwrap_or_else(|| created_at.to_string())
}

/// Render where a movement came from, in the customer's own terms (#357):
/// either they let themselves in, or a named person recorded it for them.
///
/// The two facts are independent and both are needed. `is_door_press` says
/// HOW (it is the only self-service path), `staff_name` says WHO — a name on
/// its own could not distinguish a desk entry from a door press, and a door
/// flag on its own leaves every other movement unattributed, which is
/// exactly the gap this closes.
///
/// Falls back to nothing when neither holds, rather than guessing: prod has
/// one such legacy row and an invented label on it would be a lie.
fn recent_tx_source(is_door_press: bool, staff_name: Option<&str>, lang: Lang) -> String {
    if is_door_press {
        return format!(" \u{b7} {}", i18n::t(lang, "entry_source_door"));
    }
    match staff_name {
        Some(n) if !n.is_empty() => format!(" \u{b7} {}", tf(lang, "entry_source_staff", &[n])),
        _ => String::new(),
    }
}

/// Render a recent-transaction row's sub-note (the small text under the
/// action label). Door-entry notes are stored as English "door: Nth"
/// (door.rs) — localize the DISPLAY only, the stored value stays intact.
///
/// #328: the special door-relabeling branch is gated on `is_door_press` (a
/// dedicated column only door.rs ever sets), NOT on the note text alone —
/// a staff-edited note that happens to start with "door: " on an UNRELATED
/// transaction must render as a plain note, not a misleading "door
/// re-entry" label. (Neither door.rs's same-day count nor the admin note
/// editor read this note text for classification anymore — both were
/// switched to `is_door_press` too.) Extracted for testability — see #242's
/// `format_tx_date_label` for the same pattern in this file.
fn recent_tx_sub_note(note: Option<&str>, is_door_press: bool, lang: Lang) -> String {
    match note {
        Some(n) if is_door_press && n.starts_with("door: ") => {
            let count: String = n["door: ".len()..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if count.is_empty() {
                format!(" \u{b7} {n}")
            } else {
                format!(" \u{b7} {}", tf(lang, "door_note_reentry", &[&count]))
            }
        }
        Some(n) if !n.is_empty() => format!(" \u{b7} {n}"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    // No wasm_bindgen_test_configure! — CI uses wasm-pack test --node (not browser).

    // #242: created_at is a UTC instant. Near midnight Bratislava-local, the
    // raw UTC date token is one day BEHIND the local wall date — the
    // customer's OWN transaction history would show the wrong day.
    #[wasm_bindgen_test]
    fn format_tx_date_label_midnight_boundary_resolves_bratislava_local_date() {
        // UTC 2026-07-20 22:30:00 = Bratislava-local 2026-07-21 00:30 (CEST).
        assert_eq!(
            format_tx_date_label("2026-07-20 22:30:00", Lang::Sk),
            "21.07.",
            "must resolve to the Bratislava-LOCAL calendar date, not the raw UTC token"
        );
    }

    #[wasm_bindgen_test]
    fn format_tx_date_label_agrees_with_utc_token_away_from_midnight() {
        assert_eq!(
            format_tx_date_label("2026-07-20 12:00:00", Lang::Sk),
            "20.07."
        );
    }

    /// #357: a door press is the customer's own doing — it says so, and
    /// never names a recorder.
    #[wasm_bindgen_test]
    fn recent_tx_source_door_press_says_the_customer_opened_it() {
        assert_eq!(
            recent_tx_source(true, None, Lang::Sk),
            " \u{b7} Otvoril si dvere"
        );
    }

    /// A desk-recorded movement names the person. This is the whole point of
    /// #357: before it, this row and a door press rendered identically.
    #[wasm_bindgen_test]
    fn recent_tx_source_names_the_staff_member() {
        assert_eq!(
            recent_tx_source(false, Some("Stefan"), Lang::Sk),
            " \u{b7} Zapisal Stefan"
        );
        assert_eq!(
            recent_tx_source(false, Some("Stefan"), Lang::En),
            " \u{b7} Recorded by Stefan"
        );
    }

    /// `is_door_press` WINS over a stray name. A door row carries no
    /// `staff_id` in practice, but if one ever appeared the customer must
    /// still be told they opened the door themselves — not that somebody
    /// else recorded their own entry for them.
    #[wasm_bindgen_test]
    fn recent_tx_source_door_press_wins_over_a_staff_name() {
        assert_eq!(
            recent_tx_source(true, Some("Stefan"), Lang::Sk),
            " \u{b7} Otvoril si dvere"
        );
    }

    /// Neither flag nor name (prod has exactly one such legacy row): render
    /// NOTHING. Inventing a source would be a lie about the customer's own
    /// history.
    #[wasm_bindgen_test]
    fn recent_tx_source_is_empty_when_nothing_is_known() {
        assert_eq!(recent_tx_source(false, None, Lang::Sk), "");
        assert_eq!(
            recent_tx_source(false, Some(""), Lang::Sk),
            "",
            "an empty name is not a recorder"
        );
    }

    /// A genuine door press (is_door_press=true) renders the localized
    /// "door re-entry" label, not the raw English note text.
    #[wasm_bindgen_test]
    fn recent_tx_sub_note_renders_localized_label_for_a_genuine_door_press() {
        assert_eq!(
            recent_tx_sub_note(Some("door: 1st"), true, Lang::Sk),
            " \u{b7} Vstup c. 1"
        );
        assert_eq!(
            recent_tx_sub_note(Some("door: 1st"), true, Lang::En),
            " \u{b7} Entry #1"
        );
    }

    /// #328 — a `"door: "`-prefixed note on a row that was NOT actually
    /// authored by the door route (is_door_press=false — e.g. a staff note
    /// edit on an unrelated transaction) must render the RAW note text, not
    /// the misleading localized door-re-entry label. This is the frontend
    /// half of the same corruption vector the server-side regression tests
    /// (`door_route.rs`'s `staff_note_edit_starting_with_door_prefix_...`,
    /// `monthly_pass.rs`'s `..._is_manual_despite_door_prefixed_note`) cover
    /// for billing/audit classification.
    #[wasm_bindgen_test]
    fn recent_tx_sub_note_shows_raw_note_when_door_prefixed_but_is_door_press_is_false() {
        assert_eq!(
            recent_tx_sub_note(Some("door: 1st"), false, Lang::Sk),
            " \u{b7} door: 1st",
            "a door-prefixed note with is_door_press=false must NOT get the \
             localized re-entry label — the customer must see the honest raw note"
        );
    }

    #[wasm_bindgen_test]
    fn recent_tx_sub_note_shows_plain_note_when_unrelated() {
        assert_eq!(
            recent_tx_sub_note(Some("refreshments"), false, Lang::Sk),
            " \u{b7} refreshments"
        );
    }

    #[wasm_bindgen_test]
    fn recent_tx_sub_note_is_empty_when_no_note() {
        assert_eq!(recent_tx_sub_note(None, false, Lang::Sk), "");
        assert_eq!(recent_tx_sub_note(Some(""), false, Lang::Sk), "");
    }
}
