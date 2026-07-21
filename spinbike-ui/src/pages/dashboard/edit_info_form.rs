use leptos::prelude::*;
use spinbike_core::auth::Role;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;

use crate::api;
use crate::auth;
use crate::components::Sheet;
use crate::i18n::{self, Lang};

use super::CardInfo;
use super::deleted_email_conflict::DeletedEmailConflictDialog;
use super::helpers::event_target_value;

/// Non-empty-after-trim → `Some`, else `None` (name/email field semantics).
fn nz_trim(s: String) -> Option<String> {
    if s.trim().is_empty() { None } else { Some(s) }
}

/// Non-empty (no trim) → `Some`, else `None` (company/phone/password semantics —
/// preserves the exact per-field behavior the Save handler always had).
fn nz(s: String) -> Option<String> {
    if s.is_empty() { None } else { Some(s) }
}

/// PUT the edit-form fields to `/api/users/{id}`. Shared by Save AND by the
/// save-then-invite path so both persist IDENTICAL field semantics — the invite
/// endpoint reads the committed DB row, so the email must be saved before it can
/// be invited against.
async fn save_user_fields(
    card_id: i64,
    name: String,
    email: String,
    company: String,
    phone: String,
    allow_self_entry: Option<bool>,
    password: String,
) -> Result<CardInfo, api::ApiError> {
    #[derive(serde::Serialize)]
    struct Req {
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        email: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        company: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        phone: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        allow_self_entry: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        password: Option<String>,
    }
    let req = Req {
        name: nz_trim(name),
        email: nz_trim(email),
        company: nz(company),
        phone: nz(phone),
        allow_self_entry,
        password: nz(password),
    };
    api::put_json::<Req, CardInfo>(&format!("/api/users/{card_id}"), &req).await
}

/// Format a save/PUT error for the IN-SHEET red alert (shared by Save and by the
/// save step of save-then-invite). Names the colliding account for staff/admin
/// (the server withholds that identity from a self-editing customer).
fn save_error_text(lang: Lang, e: api::ApiError) -> String {
    if let Some(name) = e.conflict_name {
        let who = match e.conflict_card {
            Some(card) if !card.trim().is_empty() => format!("{name} ({card})"),
            _ => name,
        };
        i18n::tf(lang, "email_already_used_by", &[&who])
    } else if e.message.contains("email already exists") {
        // Generic (no-name) collision — the branch a CUSTOMER self-edit hits.
        // Couples to the server's English 409 text; falls through to the raw
        // formatted server text below if that wording ever changes (never a
        // break, just less specific).
        i18n::t(lang, "email_already_used").to_string()
    } else {
        i18n::tf(lang, "error_format", &[&e.message])
    }
}

/// Admin user-edit form. Refreshes its inputs on EVERY reopen from the
/// authoritative server state via `GET /api/users/lookup/{card_code}` and
/// writes values directly to the input elements via `NodeRef`. Bypasses
/// `prop:value` so re-rendering doesn't lose the user's typed input mid-edit
/// and a server refetch DOES override stale signal state.
///
/// Field-level guards on the server:
/// - `allow_self_entry`: admin-only.
/// - `password`: admin OR self.
#[component]
pub fn EditInfoForm(
    card: CardInfo,
    set_selected: WriteSignal<Option<CardInfo>>,
    set_msg: WriteSignal<String>,
    /// Shared dashboard red-alert channel (#126). No longer used by
    /// save/invite failures inside this form (both now render in the
    /// IN-SHEET `save_err` alert instead, #232) — kept as a prop only so
    /// `run_save` can defensively clear any STALE alert left by another
    /// dashboard action (e.g. Delete/Block) before this action resolves.
    set_err: WriteSignal<String>,
    /// Signal controlling visibility — the parent sets it to false to hide.
    show: Signal<bool>,
    /// Called when the sheet should close (cancel or save success).
    #[prop(into)]
    on_close: Callback<()>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let card_id = card.id;
    let initial_code = card.card_code.clone();
    // Initial values are written into the inputs via NodeRef.set_value
    // inside the refresh Effect (see below) — we don't pass `value=` at
    // the macro level because the move closure that wraps the sheet must
    // stay FnMut across re-renders, and `value=expr.clone()` made it
    // FnOnce. The Effect runs on the first show=true with prev=None and
    // populates inputs from the latest server state.
    let initial_allow_se = card.allow_self_entry;
    // Target user's role — used to hide `allow_self_entry` when editing
    // an admin/staff row (those roles bypass the flag, so an "off"
    // checkbox confuses the operator — #94).
    let target_is_customer = !matches!(card.role, Some(Role::Admin | Role::Staff));
    // Initial values from the card prop, exposed as ReadSignals so the
    // outer `move ||` closure stays Fn (signals are Copy). Inputs use
    // `value=<sig>.get()` which evaluates fresh on every render.
    let (initial_name, _) = signal(card.name.clone());
    let (initial_email, _) = signal(card.email.clone().unwrap_or_default());
    let (initial_company, _) = signal(card.company.clone().unwrap_or_default());
    let (initial_phone, _) = signal(card.phone.clone().unwrap_or_default());
    // Reactive mirror of the CURRENT email field (typed OR saved) — the
    // "Poslat pozvanku" button is enabled whenever this is non-empty, so a
    // freshly typed email enables it immediately (no pre-save/reopen, #141).
    // Kept in sync by `on:input` on the email input and by the refresh Effect's
    // smart-overwrite below (which writes via NodeRef and so must sync this
    // signal explicitly — `set_value()` emits no `input` event). Seeded from
    // the card's persisted email. Save/invite are still mutually exclusive
    // while in flight (both gate on the other's loading flag, #111).
    let (email_sig, set_email_sig) = signal(card.email.clone().unwrap_or_default());

    // NodeRefs declared at the function-body level so the refresh Effect
    // can write to them directly when fetch completes. They're populated
    // when the sheet mounts (inside the show=true branch); the Effect
    // checks `get_untracked()` for None and no-ops in that case.
    let name_ref = NodeRef::<leptos::html::Input>::new();
    let email_ref = NodeRef::<leptos::html::Input>::new();
    let company_ref = NodeRef::<leptos::html::Input>::new();
    let phone_ref = NodeRef::<leptos::html::Input>::new();
    let password_ref = NodeRef::<leptos::html::Input>::new();
    let (allow_self_entry, set_allow_self_entry) = signal(initial_allow_se);

    // Read the caller's role to gate the admin-only fields client-side.
    let is_admin = auth::get_user().map(|u| u.role.is_admin()).unwrap_or(false);

    // Refresh from server every time show transitions false→true. Sets the
    // input values via NodeRef + HtmlInputElement::set_value, so the latest
    // saved data is what the user sees on reopen.
    // Refresh on every show=true transition (including first open).
    // SMART OVERWRITE: only writes a fetched value to the input if the
    // input currently still holds the initial/expected value — i.e. the
    // user hasn't typed anything new. This avoids the race where the
    // fetch overwrites a user-typed value mid-edit (which previously
    // sent the user's keystrokes back to "Original Name" before save).
    let lookup_code = initial_code.clone();
    let initial_name_for_eff = initial_name;
    let initial_email_for_eff = initial_email;
    let initial_company_for_eff = initial_company;
    let initial_phone_for_eff = initial_phone;
    Effect::new(move |prev_shown: Option<bool>| {
        let now_shown = show.get();
        if !now_shown {
            return now_shown;
        }
        // No-op on the initial run when show is already false (most cases).
        if prev_shown == Some(true) {
            return now_shown; // re-triggered by other tracked signal, not a transition
        }
        let code = lookup_code.clone();
        if let Some(code) = code {
            let initial_name = initial_name_for_eff.get_untracked();
            let initial_email = initial_email_for_eff.get_untracked();
            let initial_company = initial_company_for_eff.get_untracked();
            let initial_phone = initial_phone_for_eff.get_untracked();
            spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(0).await;
                if let Ok(c) = api::get::<CardInfo>(&format!("/api/users/lookup/{code}")).await {
                    // Overwrite only if the input's current DOM value matches
                    // the initial rendered value (user hasn't typed anything).
                    // Otherwise the user is mid-edit; leave it alone.
                    let smart_set =
                        |nr: &NodeRef<leptos::html::Input>, initial: &str, new_val: &str| {
                            if let Some(el) = nr.get_untracked() {
                                let input: &HtmlInputElement = &el;
                                let cur = input.value();
                                if cur == initial {
                                    input.set_value(new_val);
                                }
                            }
                        };
                    smart_set(&name_ref, &initial_name, &c.name);
                    // Email: smart-overwrite the DOM value AND keep the reactive
                    // gate signal (`email_sig`) in sync — but ONLY when the user
                    // hasn't typed (cur == initial), mirroring smart_set. This
                    // is what re-enables the invite button after a
                    // Cancel-then-reopen against an out-of-band email change:
                    // `set_value()` fires no `input` event, so `on:input` won't
                    // run — sync `email_sig` explicitly here.
                    if let Some(el) = email_ref.get_untracked() {
                        let input: &HtmlInputElement = &el;
                        if input.value() == initial_email {
                            let new_email = c.email.as_deref().unwrap_or("");
                            input.set_value(new_email);
                            set_email_sig.set(new_email.to_string());
                        }
                    }
                    smart_set(
                        &company_ref,
                        &initial_company,
                        c.company.as_deref().unwrap_or(""),
                    );
                    smart_set(&phone_ref, &initial_phone, c.phone.as_deref().unwrap_or(""));
                    // Checkbox uses a signal, not NodeRef — safe to always sync
                    // (user can re-toggle if they want a different value).
                    // Skipped for admin/staff targets: the row isn't rendered
                    // for them, so syncing a hidden signal would be wasted
                    // reactive work and leave a stale value behind a hidden
                    // control (#94 review item #4).
                    if target_is_customer {
                        set_allow_self_entry.set(c.allow_self_entry);
                    }
                    // (The invite button's gating value — `email_sig` — is
                    // re-synced in the email smart-overwrite block above.)
                }
            });
        }
        now_shown
    });

    view! {
        {move || {
            if !show.get() {
                return ().into_any();
            }

            let (loading, set_loading) = signal(false);
            // Local, IN-SHEET save-error channel. The dashboard's shared red
            // alert (mod.rs) renders BEHIND this sheet's `z-index: 200` blur
            // backdrop, so a save error routed there is invisible while the
            // sheet stays open — which is exactly what happened: a rejected
            // save (e.g. the 409 email-uniqueness conflict) set the shared
            // channel, the operator saw nothing, and it read as "it just
            // didn't save". Save keeps the sheet open (to fix inline), so its
            // error MUST live inside the sheet. Invite errors route through
            // this SAME channel now (#232) — invite no longer closes the
            // sheet on error either, see `run_save` below.
            let (save_err, set_save_err) = signal(String::new());
            // In-sheet GREEN confirmation after a successful invite (#232) —
            // the sheet no longer closes on invite completion, so success
            // needs its own in-sheet channel (paired with `save_err` above,
            // which invite failures now also route through — see
            // `run_save` below).
            let (invite_ok, set_invite_ok) = signal(String::new());
            // Stash for the CardInfo persisted by a successful invite while
            // the sheet stays open (#232). `set_selected` would rebuild this
            // whole component (the parent's `match selected.get()` — see
            // dashboard/mod.rs), wiping in-render signals/DOM state, so it
            // must NOT be called right after invite success. Instead the
            // saved value is stashed here and flushed to `set_selected` on
            // every ACTUAL close path (Cancel button, backdrop/Escape close)
            // so the dashboard card behind reflects the new email even if
            // the operator cancels without an explicit Save afterward. A
            // subsequent Save success already flushes its OWN fresher
            // CardInfo directly, so it doesn't need to consult this stash.
            let invited_saved: StoredValue<Option<CardInfo>> = StoredValue::new(None);
            let on_close_cancel = on_close;
            let on_close_btn = on_close;
            let on_close_save = on_close;
            // #143: closing the sheet after the operator restores the old account.
            let on_close_restored = on_close;
            let initial_allow_se_at_open = allow_self_entry.get_untracked();

            // Read a NodeRef input's current DOM value — shared by Save and by
            // the save-then-invite click so both collect identical field values.
            let read = |n: &NodeRef<leptos::html::Input>| {
                n.get()
                    .map(|el| {
                        let el: &HtmlInputElement = &el;
                        el.value()
                    })
                    .unwrap_or_default()
            };
            // The admin-only `allow_self_entry` payload: only sent when changed
            // AND the target is a customer (admin/staff bypass the flag, and the
            // row is hidden for them — #94). Shared by Save + save-then-invite.
            let allow_se_req = move || {
                let allow_se = allow_self_entry.get_untracked();
                if is_admin && target_is_customer && allow_se != initial_allow_se_at_open {
                    Some(allow_se)
                } else {
                    None
                }
            };
            // Collect the whole form in one place so Save and save-then-invite
            // can never drift as fields are added/removed. Returns
            // (name, email, company, phone, password, allow_self_entry).
            let collect = move || {
                (
                    read(&name_ref),
                    read(&email_ref),
                    read(&company_ref),
                    read(&phone_ref),
                    read(&password_ref),
                    allow_se_req(),
                )
            };

            let (invite_loading, set_invite_loading) = signal(false);
            // #143 — soft-deleted-email conflict during a save: (archived id,
            // name, deleted_at). When Some, the resolution dialog is shown on
            // top of this (still-open) edit sheet, so a retry can re-read the
            // same fields.
            let (conflict, set_conflict) =
                signal::<Option<(i64, String, Option<String>)>>(None);
            // Which action to re-run after the email is freed: false = Save,
            // true = Save-then-invite (#141).
            let (retry_invite, set_retry_invite) = signal(false);

            // The core save (optionally followed by invite). Shared by the Save
            // button, the save-then-invite button, and the #143 free-email
            // auto-retry — so all three persist IDENTICAL field semantics.
            //
            // ONE click on the invite button = persist the current field values
            // (incl. a freshly typed email) FIRST, then invite: the invite
            // endpoint reads the committed DB row (#141).
            let run_save: Callback<bool> = Callback::new(move |also_invite: bool| {
                if loading.get_untracked() || invite_loading.get_untracked() {
                    return;
                }
                let (name, email, company, phone, password, allow_self_entry) = collect();

                // Clear any stale alert from a previous action before this one
                // resolves — otherwise a stale red error (or green success) from
                // an earlier action can still be showing when this one completes,
                // stacking two conflicting alerts (#126 follow-up).
                set_msg.set(String::new());
                set_err.set(String::new());
                set_save_err.set(String::new());
                set_invite_ok.set(String::new());
                if also_invite {
                    set_invite_loading.set(true);
                } else {
                    set_loading.set(true);
                }
                spawn_local(async move {
                    // Step 1 — persist. A save failure keeps the sheet OPEN to
                    // fix inline (never invites on a failed save).
                    let saved = match save_user_fields(
                        card_id, name, email, company, phone, allow_self_entry, password,
                    )
                    .await
                    {
                        Ok(c) => c,
                        Err(e) => {
                            // #143: an email held by a SOFT-DELETED account opens
                            // the restore / free-email dialog instead of a
                            // dead-end error. The sheet stays open so the retry
                            // re-reads the same fields. Any other error → the
                            // usual in-sheet red alert.
                            if let Some(c) = e.deleted_email_conflict() {
                                set_retry_invite.set(also_invite);
                                set_conflict.set(Some(c));
                            } else {
                                set_save_err.set(save_error_text(lang.get_untracked(), e));
                            }
                            if also_invite {
                                set_invite_loading.set(false);
                            } else {
                                set_loading.set(false);
                            }
                            return;
                        }
                    };
                    set_email_sig.set(saved.email.clone().unwrap_or_default());

                    if !also_invite {
                        // Save-only success. ORDER MATTERS (reactive_graph
                        // 0.1.8): the LOCAL `loading` signal FIRST, then the
                        // disposal triggers. `set_selected` rebuilds this
                        // component (parent tracks `selected`) and `on_close`
                        // hides the sheet — both dispose this scope
                        // synchronously, and touching a local/component signal
                        // AFTERWARD panics ("access a reactive value that has
                        // already been disposed") — a WASM RuntimeError in the
                        // console (a Playwright clean-console failure). Local
                        // first, dispose last.
                        set_loading.set(false);
                        set_selected.set(Some(saved));
                        set_msg.set(i18n::t(lang.get_untracked(), "saved").to_string());
                        on_close_save.run(());
                        return;
                    }

                    // Step 2 — invite against the now-committed email.
                    #[derive(serde::Deserialize)]
                    struct InviteResponse {
                        sent_to: String,
                    }
                    match api::post::<(), InviteResponse>(
                        &format!("/api/users/{card_id}/invite"),
                        &(),
                    )
                    .await
                    {
                        Ok(resp) => {
                            // #232: the sheet STAYS OPEN on invite success —
                            // stash the already-committed CardInfo instead of
                            // flushing it to `set_selected` now (that would
                            // rebuild/dispose this component — see the
                            // `invited_saved` comment above). The in-sheet
                            // green alert replaces the old shared-channel
                            // message + auto-close.
                            invited_saved.set_value(Some(saved));
                            set_invite_ok.set(i18n::tf(
                                lang.get_untracked(),
                                "invite_sent_in_sheet",
                                &[&resp.sent_to],
                            ));
                        }
                        Err(e) => {
                            // #232: routed to the IN-SHEET red alert (same
                            // channel Save failures use) instead of the
                            // shared dashboard channel — the sheet no longer
                            // closes on invite error either, so the operator
                            // can fix the email inline and retry.
                            if e == "mail_not_configured" {
                                set_save_err.set(
                                    i18n::t(lang.get_untracked(), "invite_mail_not_configured")
                                        .to_string(),
                                );
                            } else {
                                set_save_err
                                    .set(i18n::tf(lang.get_untracked(), "error_format", &[&e]));
                            }
                        }
                    }

                    // Local only now — invite no longer disposes this scope
                    // on either outcome (#232), so there's no ordering
                    // constraint here anymore.
                    set_invite_loading.set(false);
                });
            });

            let on_submit = move |ev: web_sys::SubmitEvent| {
                ev.prevent_default();
                run_save.run(false);
            };

            let on_invite_click = move |_: web_sys::MouseEvent| {
                run_save.run(true);
            };

            view! {
                <Sheet
                    on_close=Callback::new(move |()| {
                        // Block backdrop-click / Escape while a save OR an
                        // invite is in flight — closing here would tear down
                        // this reactive scope (loading/invite_loading/
                        // on_submit/on_invite_click are all created in the
                        // enclosing `move ||`) out from under the pending
                        // spawn_local, which is exactly the disposed-closure
                        // class of bug this Sheet already hit once (see #89).
                        // `loading` matters specifically for Save: a close
                        // during the PUT window would dispose the scope, so a
                        // 409's `set_save_err` would no-op on a disposed
                        // signal and the failure would surface NOTHING (the
                        // in-sheet error is now the only channel — the shared
                        // dashboard alert is occluded by this backdrop).
                        // Symmetric with the Cancel/Save buttons, both already
                        // disabled on `loading`.
                        if !invite_loading.get_untracked() && !loading.get_untracked() {
                            // #232: flush a stashed post-invite CardInfo to
                            // the dashboard BEFORE closing, so a backdrop/
                            // Escape close after a successful invite doesn't
                            // leave the card behind showing the old email.
                            if let Some(saved) = invited_saved.get_value() {
                                set_selected.set(Some(saved));
                            }
                            on_close_cancel.run(());
                        }
                    })
                    title=i18n::t(lang.get(), "edit_info").to_string()
                    testid="sheet-edit-info"
                >
                    <form on:submit=on_submit>
                        <div class="form-group">
                            <label>{i18n::t(lang.get(), "name")}</label>
                            <input
                                type="text"
                                class="form-control"
                                node_ref=name_ref
                                value=initial_name.get_untracked()
                            />
                        </div>
                        <div class="form-group">
                            <label>{i18n::t(lang.get(), "email")}</label>
                            <input
                                type="email"
                                class="form-control"
                                node_ref=email_ref
                                value=initial_email.get_untracked()
                                on:input=move |ev| set_email_sig.set(event_target_value(&ev))
                            />
                        </div>
                        <div class="form-group">
                            <button
                                type="button"
                                class="btn btn--ghost"
                                data-testid="user-edit-send-invite"
                                disabled=move || {
                                    // Enabled as soon as the CURRENT email field
                                    // is non-empty — the click saves that email
                                    // before inviting (#141), so no pre-save is
                                    // needed. Still blocked while a Save OR an
                                    // invite is in flight: mutually exclusive
                                    // with the Save button (both gate on the
                                    // other's loading flag), so two overlapping
                                    // PUTs can't race (#111).
                                    invite_loading.get()
                                        || loading.get()
                                        || email_sig.get().trim().is_empty()
                                }
                                on:click=on_invite_click
                            >
                                {move || i18n::t(lang.get(), "send_invite")}
                            </button>
                        </div>
                        <div class="form-group">
                            <label>{i18n::t(lang.get(), "company")}</label>
                            <input
                                type="text"
                                class="form-control"
                                node_ref=company_ref
                                value=initial_company.get_untracked()
                            />
                        </div>
                        <div class="form-group">
                            <label>{i18n::t(lang.get(), "phone")}</label>
                            <input
                                type="text"
                                class="form-control"
                                node_ref=phone_ref
                                value=initial_phone.get_untracked()
                            />
                        </div>
                        {if is_admin {
                            let allow_se_row = if target_is_customer {
                                view! {
                                    <label class="form-row" data-testid="user-edit-allow-self-entry-row">
                                        <input
                                            type="checkbox"
                                            data-testid="user-edit-allow-self-entry"
                                            prop:checked=move || allow_self_entry.get()
                                            on:change=move |ev| {
                                                let el: HtmlInputElement =
                                                    ev.target().unwrap().unchecked_into();
                                                set_allow_self_entry.set(el.checked());
                                            }
                                        />
                                        <span>{move || i18n::t(lang.get(), "admin_allow_self_entry")}</span>
                                        <small class="form-help">
                                            {move || i18n::t(lang.get(), "admin_allow_self_entry_help")}
                                        </small>
                                    </label>
                                }.into_any()
                            } else {
                                ().into_any()
                            };
                            // Password is a LOGIN credential. Customers are
                            // passwordless (magic-link only, per the onboarding
                            // design) — a "set new password" field on a customer
                            // target is meaningless and confused the operator.
                            // Only admin/staff targets (who sign in via
                            // /api/auth/login) get the field. admin/staff targets
                            // have target_is_customer == false, so they keep it;
                            // the allow_self_entry row is the customer-only
                            // counterpart, so the two never show together.
                            let password_field = if target_is_customer {
                                ().into_any()
                            } else {
                                view! {
                                    <div class="form-group">
                                        <label>{move || i18n::t(lang.get(), "user_edit_new_password")}</label>
                                        <input
                                            type="password"
                                            class="form-control"
                                            data-testid="user-edit-password"
                                            node_ref=password_ref
                                            placeholder=move || i18n::t(lang.get(), "user_edit_new_password_placeholder")
                                            autocomplete="new-password"
                                        />
                                        <small class="form-help">
                                            {move || i18n::t(lang.get(), "user_edit_new_password_help")}
                                        </small>
                                    </div>
                                }.into_any()
                            };
                            view! {
                                {password_field}
                                {allow_se_row}
                            }.into_any()
                        } else {
                            ().into_any()
                        }}
                        {move || {
                            // In-sheet GREEN invite-success confirmation
                            // (#232) — replaces the old shared-dashboard-alert
                            // + auto-close, so the operator sees it without
                            // being thrown out of the sheet.
                            let ok = invite_ok.get();
                            if ok.is_empty() {
                                ().into_any()
                            } else {
                                view! {
                                    <div class="alert alert-success" data-testid="edit-info-invite-sent">{ok}</div>
                                }.into_any()
                            }
                        }}
                        {move || {
                            // In-sheet save error (bug: was invisible behind
                            // the sheet backdrop when routed to the shared
                            // dashboard alert). Rendered as part of the sheet
                            // content, so it sits ABOVE the backdrop. Invite
                            // errors are routed here too now (#232) since
                            // invite no longer closes the sheet on failure.
                            let e = save_err.get();
                            if e.is_empty() {
                                ().into_any()
                            } else {
                                view! {
                                    <div class="alert alert-error" data-testid="edit-info-error">{e}</div>
                                }.into_any()
                            }
                        }}
                        <div class="sheet__actions">
                            <button
                                type="button"
                                class="btn btn--ghost"
                                // Also blocked while an invite is in flight —
                                // symmetric with the Sheet's own on_close gate
                                // above; same disposed-reactive-scope reason.
                                disabled=move || loading.get() || invite_loading.get()
                                on:click=move |_| {
                                    let cb = on_close_btn;
                                    spawn_local(async move {
                                        gloo_timers::future::TimeoutFuture::new(0).await;
                                        // #232: same flush as the backdrop/Escape
                                        // close above — Cancel after a successful
                                        // invite must not leave the dashboard card
                                        // behind showing the pre-invite email.
                                        if let Some(saved) = invited_saved.get_value() {
                                            set_selected.set(Some(saved));
                                        }
                                        cb.run(());
                                    });
                                }
                            >
                                {i18n::t(lang.get(), "cancel")}
                            </button>
                            <button
                                type="submit"
                                class="btn btn--primary"
                                disabled=move || loading.get() || invite_loading.get()
                            >
                                {i18n::t(lang.get(), "save")}
                            </button>
                        </div>
                    </form>
                </Sheet>
                {move || match conflict.get() {
                    // #143 resolution dialog, rendered on top of the still-open
                    // edit sheet so a free-email retry can re-read the fields.
                    Some((cid, cname, cdel)) => view! {
                        <DeletedEmailConflictDialog
                            conflict_id=cid
                            conflict_name=cname
                            conflict_deleted_at=cdel
                            // Email freed → re-run whichever action was pending.
                            on_email_freed=Callback::new(move |()| {
                                set_conflict.set(None);
                                run_save.run(retry_invite.get_untracked());
                            })
                            // Old account restored → abandon this edit; close
                            // with a confirmation.
                            on_restored=Callback::new(move |()| {
                                set_conflict.set(None);
                                set_msg.set(
                                    i18n::t(lang.get_untracked(), "deleted_email_restored_ok")
                                        .to_string(),
                                );
                                on_close_restored.run(());
                            })
                            on_cancel=Callback::new(move |()| set_conflict.set(None))
                        />
                    }.into_any(),
                    None => ().into_any(),
                }}
            }.into_any()
        }}
    }
}
