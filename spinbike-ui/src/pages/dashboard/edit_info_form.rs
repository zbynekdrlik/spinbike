use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;

use crate::api;
use crate::components::Sheet;
use crate::i18n::{self, Lang};

use super::CardInfo;

#[component]
pub fn EditInfoForm(
    card: CardInfo,
    set_selected: WriteSignal<Option<CardInfo>>,
    set_msg: WriteSignal<String>,
    /// Signal controlling visibility — the parent sets it to false to hide.
    show: Signal<bool>,
    /// Called when the sheet should close (cancel or save success).
    #[prop(into)]
    on_close: Callback<()>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let card_id = card.id;

    // Stash non-Copy locals so the reactive mount closure (Fn) can clone them
    // cheaply per render rather than moving them once.
    let nv = StoredValue::new(card.name.clone());
    let ev = StoredValue::new(card.email.clone().unwrap_or_default());
    let cv = StoredValue::new(card.company.clone().unwrap_or_default());
    let pv = StoredValue::new(card.phone.clone().unwrap_or_default());
    let (allow_self_entry, set_allow_self_entry) = signal(card.allow_self_entry);

    view! {
        {move || {
            if !show.get() {
                return ().into_any();
            }

            let name_ref = NodeRef::<leptos::html::Input>::new();
            let email_ref = NodeRef::<leptos::html::Input>::new();
            let company_ref = NodeRef::<leptos::html::Input>::new();
            let phone_ref = NodeRef::<leptos::html::Input>::new();
            let (loading, set_loading) = signal(false);

            let on_close_cancel = on_close.clone();
            let on_close_btn = on_close.clone();
            let on_close_save = on_close.clone();

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

                set_loading.set(true);
                let on_close_inner = on_close_save.clone();
                let allow_se = allow_self_entry.get_untracked();
                spawn_local(async move {
                    #[derive(serde::Serialize)]
                    struct Req {
                        name: Option<String>,
                        email: Option<String>,
                        company: Option<String>,
                        phone: Option<String>,
                        allow_self_entry: Option<bool>,
                    }
                    let req = Req {
                        name: if name.trim().is_empty() { None } else { Some(name) },
                        email: if email.trim().is_empty() { None } else { Some(email) },
                        company: if company.is_empty() { None } else { Some(company) },
                        phone: if phone.is_empty() { None } else { Some(phone) },
                        allow_self_entry: Some(allow_se),
                    };
                    match api::put_json::<Req, CardInfo>(&format!("/api/users/{card_id}"), &req).await {
                        Ok(c) => {
                            set_selected.set(Some(c));
                            set_msg.set(i18n::t(lang.get_untracked(), "saved").to_string());
                            on_close_inner.run(());
                        }
                        Err(e) => set_msg.set(i18n::tf(
                            lang.get_untracked(),
                            "error_format",
                            &[&e],
                        )),
                    }
                    set_loading.set(false);
                });
            };

            view! {
                <Sheet
                    on_close=Callback::new(move |()| on_close_cancel.run(()))
                    title=i18n::t(lang.get(), "edit_info").to_string()
                    testid="sheet-edit-info"
                >
                    <form on:submit=on_submit>
                        <div class="form-group">
                            <label>{i18n::t(lang.get(), "name")}</label>
                            <input type="text" class="form-control" node_ref=name_ref value=nv.get_value() />
                        </div>
                        <div class="form-group">
                            <label>{i18n::t(lang.get(), "email")}</label>
                            <input type="email" class="form-control" node_ref=email_ref value=ev.get_value() />
                        </div>
                        <div class="form-group">
                            <label>{i18n::t(lang.get(), "company")}</label>
                            <input type="text" class="form-control" node_ref=company_ref value=cv.get_value() />
                        </div>
                        <div class="form-group">
                            <label>{i18n::t(lang.get(), "phone")}</label>
                            <input type="text" class="form-control" node_ref=phone_ref value=pv.get_value() />
                        </div>
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
                        <div class="sheet__actions">
                            <button
                                type="button"
                                class="btn btn--ghost"
                                disabled=move || loading.get()
                                on:click=move |_| {
                                    // Defer the close to next macrotask so the
                                    // click event finishes dispatching before
                                    // the parent's reactive tree unmounts the
                                    // sheet. See #89.
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
                                disabled=move || loading.get()
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
