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
    /// Red-alert channel (#126) — save/invite failures render here, not in
    /// the green success alert. The mail_not_configured 503 counts as an
    /// error (it means the invite was NOT sent) so it goes through this
    /// channel too, alongside the generic invite-error case.
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
    // Tracks the LAST-SAVED email (not the unsaved draft in the input) — the
    // "Poslat pozvanku" button is enabled only against this, per #111. Starts
    // from the card's current persisted email; updated from the fresh
    // `CardInfo` the save handler gets back on success (see on_submit below).
    let (saved_email, set_saved_email) = signal(card.email.clone());

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
    let is_admin = auth::get_user().map(|u| u.role == "admin").unwrap_or(false);

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
                    smart_set(&email_ref, &initial_email, c.email.as_deref().unwrap_or(""));
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
                    // Re-sync the invite button's gating value too — otherwise
                    // a Cancel-then-reopen of the SAME still-mounted sheet
                    // (no remount, so `saved_email` would otherwise hold
                    // whatever it was at the last save/mount) would leave the
                    // button's enabled state stale relative to an email
                    // change made out-of-band (another staff terminal, an
                    // import) between the two opens.
                    set_saved_email.set(c.email.clone());
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
            let on_close_cancel = on_close.clone();
            let on_close_btn = on_close.clone();
            let on_close_save = on_close.clone();
            let on_close_invite = on_close.clone();
            let initial_allow_se_at_open = allow_self_entry.get_untracked();

            let on_submit = move |ev: web_sys::SubmitEvent| {
                ev.prevent_default();
                let read = |n: &NodeRef<leptos::html::Input>| {
                    n.get()
                        .map(|el| {
                            let el: &HtmlInputElement = &el;
                            el.value()
                        })
                        .unwrap_or_default()
                };
                let name = read(&name_ref);
                let email = read(&email_ref);
                let company = read(&company_ref);
                let phone = read(&phone_ref);
                let password = read(&password_ref);
                let allow_se = allow_self_entry.get_untracked();

                // Clear any stale alert from a previous action before this
                // one resolves — otherwise a stale red error (or green
                // success) from an earlier action can still be showing
                // when this one completes, stacking two conflicting
                // alerts (#126 follow-up).
                set_msg.set(String::new());
                set_err.set(String::new());
                set_loading.set(true);
                let on_close_inner = on_close_save.clone();
                spawn_local(async move {
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
                        name: if name.trim().is_empty() { None } else { Some(name) },
                        email: if email.trim().is_empty() { None } else { Some(email) },
                        company: if company.is_empty() { None } else { Some(company) },
                        phone: if phone.is_empty() { None } else { Some(phone) },
                        // Admin-only field, only sent when changed AND the
                        // target user is a customer (admin/staff bypass the
                        // flag, and the row is hidden for them — #94).
                        allow_self_entry: if is_admin
                            && target_is_customer
                            && allow_se != initial_allow_se_at_open
                        {
                            Some(allow_se)
                        } else {
                            None
                        },
                        password: if password.is_empty() { None } else { Some(password) },
                    };
                    match api::put_json::<Req, CardInfo>(&format!("/api/users/{card_id}"), &req).await {
                        Ok(c) => {
                            set_saved_email.set(c.email.clone());
                            set_selected.set(Some(c));
                            set_msg.set(i18n::t(lang.get_untracked(), "saved").to_string());
                            on_close_inner.run(());
                        }
                        Err(e) => set_err.set(i18n::tf(
                            lang.get_untracked(),
                            "error_format",
                            &[&e],
                        )),
                    }
                    set_loading.set(false);
                });
            };

            let (invite_loading, set_invite_loading) = signal(false);
            let on_invite_click = move |_: web_sys::MouseEvent| {
                // Same stale-alert clear as on_submit above (#126 follow-up).
                set_msg.set(String::new());
                set_err.set(String::new());
                set_invite_loading.set(true);
                let on_close_after_invite = on_close_invite.clone();
                spawn_local(async move {
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
                            set_msg.set(i18n::tf(
                                lang.get_untracked(),
                                "invite_sent",
                                &[&resp.sent_to],
                            ));
                        }
                        Err(e) => {
                            if e == "mail_not_configured" {
                                set_err.set(
                                    i18n::t(lang.get_untracked(), "invite_mail_not_configured")
                                        .to_string(),
                                );
                            } else {
                                set_err.set(i18n::tf(lang.get_untracked(), "error_format", &[&e]));
                            }
                        }
                    }
                    set_invite_loading.set(false);
                    // Close the sheet on EITHER outcome so the status line
                    // (rendered by the parent dashboard, outside this
                    // component) is actually visible — the sheet's own
                    // backdrop is a full-viewport `position: fixed; z-index:
                    // 200` blur that sits above it while the sheet stays
                    // open. Unlike Save, Invite has no "fix inline and
                    // retry" flow, so there's nothing gained by staying open.
                    on_close_after_invite.run(());
                });
            };

            view! {
                <Sheet
                    on_close=Callback::new(move |()| {
                        // Block backdrop-click / Escape while an invite is
                        // in flight — closing here would tear down this
                        // reactive scope (loading/invite_loading/on_submit/
                        // on_invite_click are all created in the enclosing
                        // `move ||`) out from under the pending spawn_local,
                        // which is exactly the disposed-closure class of bug
                        // this Sheet already hit once (see #89 in its own
                        // doc comment on `close_backdrop`/`close_keyboard`).
                        if !invite_loading.get_untracked() {
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
                            />
                        </div>
                        <div class="form-group">
                            <button
                                type="button"
                                class="btn btn--ghost"
                                data-testid="user-edit-send-invite"
                                disabled=move || {
                                    // Also gated on the SAVE form's own
                                    // `loading` (not just `invite_loading`):
                                    // without it, a user could click Save
                                    // (new email typed) then immediately
                                    // click Send before the PUT resolves —
                                    // `saved_email` wouldn't reflect the new
                                    // value yet, but the invite would still
                                    // fire (the server reads the CURRENT
                                    // DB row at request time, independent of
                                    // this signal — see #111).
                                    invite_loading.get()
                                        || loading.get()
                                        || saved_email
                                            .get()
                                            .as_deref()
                                            .filter(|s| !s.trim().is_empty())
                                            .is_none()
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
                                {allow_se_row}
                            }.into_any()
                        } else {
                            ().into_any()
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
                                    let cb = on_close_btn.clone();
                                    spawn_local(async move {
                                        gloo_timers::future::TimeoutFuture::new(0).await;
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
            }.into_any()
        }}
    }
}
