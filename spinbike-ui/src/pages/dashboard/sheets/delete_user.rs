use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::components::Sheet;
use crate::i18n::{self, Lang};

#[component]
pub fn DeleteUserSheet(
    /// Whether the sheet is visible.
    show: RwSignal<bool>,
    /// User id to soft-delete via DELETE /api/users/{id}.
    user_id: i64,
    /// User's display name (used in modal title).
    name: String,
    /// Current credit balance — warning row appears if non-zero.
    balance: f64,
    /// Active permanentka end date if any — warning row appears when Some.
    active_pass_end: Option<chrono::NaiveDate>,
    /// Invoked after a successful delete so the parent can refresh / close panel.
    on_saved: Callback<()>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");

    view! {
        {move || {
            if !show.get() {
                return ().into_any();
            }

            // Per-mount form state — every open of the sheet starts fresh.
            let (err, set_err) = signal(String::new());
            let (saving, set_saving) = signal(false);
            let name_inner = name.clone();

            let on_confirm = move |_| {
                set_err.set(String::new());
                set_saving.set(true);
                spawn_local(async move {
                    match api::delete(&format!("/api/users/{user_id}")).await {
                        Ok(()) => {
                            show.set(false);
                            on_saved.run(());
                        }
                        Err(e) => set_err.set(e),
                    }
                    set_saving.set(false);
                });
            };
            let on_cancel = move |_| {
                set_err.set(String::new());
                show.set(false);
            };

            let title = i18n::t(lang.get(), "delete_user_confirm_title")
                .replace("{name}", &name_inner);

            view! {
                <Sheet
                    on_close=Callback::new(move |()| show.set(false))
                    title=title
                    testid="sheet-delete-user"
                >
                    <p>{i18n::t(lang.get(), "delete_user_confirm_body")}</p>
                    {move || {
                        if balance.abs() < 0.005 {
                            ().into_any()
                        } else {
                            let txt = i18n::t(lang.get(), "delete_user_warning_balance")
                                .replace("{amount}", &format!("{:+.2}", balance));
                            view! {
                                <div class="alert alert-info" data-testid="delete-user-warning-balance">{txt}</div>
                            }.into_any()
                        }
                    }}
                    {move || {
                        match active_pass_end {
                            Some(d) => {
                                let txt = i18n::t(lang.get(), "delete_user_warning_pass")
                                    .replace("{date}", &d.format("%d.%m.%Y").to_string());
                                view! {
                                    <div class="alert alert-info" data-testid="delete-user-warning-pass">{txt}</div>
                                }.into_any()
                            }
                            None => ().into_any(),
                        }
                    }}
                    {move || {
                        let e = err.get();
                        if e.is_empty() {
                            view! { <div></div> }.into_any()
                        } else {
                            view! { <div class="alert alert-error" data-testid="delete-user-error">{e}</div> }.into_any()
                        }
                    }}
                    <div class="sheet__actions">
                        <button
                            class="btn btn--ghost"
                            disabled=move || saving.get()
                            on:click=on_cancel
                            data-testid="delete-user-cancel"
                        >
                            {i18n::t(lang.get(), "delete_user_cancel")}
                        </button>
                        <button
                            class="btn btn--danger"
                            disabled=move || saving.get()
                            on:click=on_confirm
                            data-testid="delete-user-confirm"
                        >
                            {i18n::t(lang.get(), "delete_user_confirm")}
                        </button>
                    </div>
                </Sheet>
            }.into_any()
        }}
    }
}
