//! #143 — staff resolution dialog for the soft-deleted-email conflict.
//!
//! Shown when a create/update is rejected with the
//! `email_belongs_to_deleted_account` 409: instead of an opaque error the desk
//! gets a clear message naming the ARCHIVED account that holds the address, and
//! two explicit, well-defined actions:
//!   - **Obnovit ucet** (restore) — un-delete the old account (its history and
//!     credit come back); the pending create/update is abandoned.
//!   - **Uvolnit email** (free the email) — clear the email on the archived
//!     account; the caller then retries the original create/update, which now
//!     succeeds because the address is free.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::components::Sheet;
use crate::i18n::{self, Lang};

#[component]
pub fn DeletedEmailConflictDialog(
    /// Id of the soft-deleted account holding the email (server `conflict_id`).
    conflict_id: i64,
    /// Display name of that account (server `conflict_name`).
    conflict_name: String,
    /// Raw server deletion timestamp (server `conflict_deleted_at`), or `None`.
    conflict_deleted_at: Option<String>,
    /// Invoked after "Uvolnit email" succeeds — the caller retries the original
    /// create/update now that the address is free.
    #[prop(into)]
    on_email_freed: Callback<()>,
    /// Invoked after "Obnovit ucet" succeeds — the caller closes / refreshes.
    #[prop(into)]
    on_restored: Callback<()>,
    /// Invoked on cancel (backdrop / Escape / Cancel button).
    #[prop(into)]
    on_cancel: Callback<()>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    // One in-flight guard shared by both actions — they are mutually exclusive
    // and the dialog must not be dismissible while a POST is pending (same
    // disposed-scope class of bug the edit sheet guards against, #89).
    let (busy, set_busy) = signal(false);
    let (err, set_err) = signal(String::new());

    // Body text: name + the formatted deletion date when available.
    let name_for_body = conflict_name;
    let deleted_at_for_body = conflict_deleted_at;
    let body_text = move || {
        let l = lang.get();
        let name = name_for_body.clone();
        match deleted_at_for_body
            .as_deref()
            .and_then(crate::dates::parse_server_date)
        {
            Some(d) => i18n::tf(
                l,
                "deleted_email_conflict_body",
                &[&name, &i18n::fmt_date(d, l)],
            ),
            None => i18n::tf(l, "deleted_email_conflict_body_nodate", &[&name]),
        }
    };

    let on_restore = move |_: web_sys::MouseEvent| {
        if busy.get_untracked() {
            return;
        }
        set_err.set(String::new());
        set_busy.set(true);
        let cb = on_restored;
        spawn_local(async move {
            match api::post_json::<(), serde_json::Value>(
                &format!("/api/users/{conflict_id}/restore"),
                &(),
            )
            .await
            {
                Ok(_) => {
                    set_busy.set(false);
                    cb.run(());
                }
                Err(e) => {
                    set_err.set(e.message);
                    set_busy.set(false);
                }
            }
        });
    };

    let on_free = move |_: web_sys::MouseEvent| {
        if busy.get_untracked() {
            return;
        }
        set_err.set(String::new());
        set_busy.set(true);
        let cb = on_email_freed;
        spawn_local(async move {
            match api::post_json::<(), serde_json::Value>(
                &format!("/api/users/{conflict_id}/free-email"),
                &(),
            )
            .await
            {
                Ok(_) => {
                    set_busy.set(false);
                    cb.run(());
                }
                Err(e) => {
                    set_err.set(e.message);
                    set_busy.set(false);
                }
            }
        });
    };

    let on_cancel_click = move |_: web_sys::MouseEvent| {
        if !busy.get_untracked() {
            on_cancel.run(());
        }
    };
    let on_close_cb = on_cancel;

    view! {
        <Sheet
            on_close=Callback::new(move |()| {
                if !busy.get_untracked() {
                    on_close_cb.run(());
                }
            })
            title=i18n::t(lang.get(), "deleted_email_conflict_title").to_string()
            testid="sheet-deleted-email-conflict"
        >
            <p data-testid="deleted-email-conflict-body">{body_text}</p>
            <div class="form-group">
                <button
                    type="button"
                    class="btn btn--primary"
                    data-testid="deleted-email-restore"
                    disabled=move || busy.get()
                    on:click=on_restore
                >
                    {move || i18n::t(lang.get(), "deleted_email_restore")}
                </button>
                <small class="form-help">
                    {move || i18n::t(lang.get(), "deleted_email_restore_help")}
                </small>
            </div>
            <div class="form-group">
                <button
                    type="button"
                    class="btn btn--ghost"
                    data-testid="deleted-email-free"
                    disabled=move || busy.get()
                    on:click=on_free
                >
                    {move || i18n::t(lang.get(), "deleted_email_free")}
                </button>
                <small class="form-help">
                    {move || i18n::t(lang.get(), "deleted_email_free_help")}
                </small>
            </div>
            {move || {
                let e = err.get();
                if e.is_empty() {
                    ().into_any()
                } else {
                    view! {
                        <div class="alert alert-error" data-testid="deleted-email-conflict-error">
                            {e}
                        </div>
                    }
                    .into_any()
                }
            }}
            <div class="sheet__actions">
                <button
                    type="button"
                    class="btn btn--ghost"
                    disabled=move || busy.get()
                    on:click=on_cancel_click
                >
                    {move || i18n::t(lang.get(), "cancel")}
                </button>
            </div>
        </Sheet>
    }
}
