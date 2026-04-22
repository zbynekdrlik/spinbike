use leptos::prelude::*;
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
    let first_ref = NodeRef::<leptos::html::Input>::new();
    let last_ref = NodeRef::<leptos::html::Input>::new();
    let company_ref = NodeRef::<leptos::html::Input>::new();
    let phone_ref = NodeRef::<leptos::html::Input>::new();
    let (loading, set_loading) = signal(false);

    let card_id = card.id;
    let fv = card.first_name.clone().unwrap_or_default();
    let lv = card.last_name.clone().unwrap_or_default();
    let cv = card.company.clone().unwrap_or_default();
    let pv = card.phone.clone().unwrap_or_default();

    let on_close_cancel = on_close.clone();
    let on_close_btn = on_close.clone();

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
        let first = read(&first_ref);
        let last = read(&last_ref);
        let company = read(&company_ref);
        let phone = read(&phone_ref);

        set_loading.set(true);
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req {
                first_name: Option<String>,
                last_name: Option<String>,
                company: Option<String>,
                phone: Option<String>,
            }
            let req = Req {
                first_name: if first.is_empty() { None } else { Some(first) },
                last_name: if last.is_empty() { None } else { Some(last) },
                company: if company.is_empty() {
                    None
                } else {
                    Some(company)
                },
                phone: if phone.is_empty() { None } else { Some(phone) },
            };
            match api::put_json::<Req, CardInfo>(&format!("/api/cards/{card_id}"), &req).await {
                Ok(c) => {
                    set_selected.set(Some(c));
                    set_msg.set(i18n::t(lang.get_untracked(), "saved").to_string());
                    on_close.run(());
                }
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
            set_loading.set(false);
        });
    };

    view! {
        <Sheet
            show=show
            on_close=Callback::new(move |()| on_close_cancel.run(()))
            title=i18n::t(lang.get_untracked(), "edit_info").to_string()
            testid="sheet-edit-info"
        >
            <form on:submit=on_submit>
                <div class="form-group">
                    <label>{move || i18n::t(lang.get(), "first_name")}</label>
                    <input type="text" class="form-control" node_ref=first_ref value=fv />
                </div>
                <div class="form-group">
                    <label>{move || i18n::t(lang.get(), "last_name")}</label>
                    <input type="text" class="form-control" node_ref=last_ref value=lv />
                </div>
                <div class="form-group">
                    <label>{move || i18n::t(lang.get(), "company")}</label>
                    <input type="text" class="form-control" node_ref=company_ref value=cv />
                </div>
                <div class="form-group">
                    <label>{move || i18n::t(lang.get(), "phone")}</label>
                    <input type="text" class="form-control" node_ref=phone_ref value=pv />
                </div>
                <div class="sheet__actions">
                    <button
                        type="button"
                        class="btn btn--ghost"
                        disabled=move || loading.get()
                        on:click=move |_| on_close_btn.run(())
                    >
                        {move || i18n::t(lang.get(), "cancel")}
                    </button>
                    <button
                        type="submit"
                        class="btn btn--primary"
                        disabled=move || loading.get()
                    >
                        {move || i18n::t(lang.get(), "save")}
                    </button>
                </div>
            </form>
        </Sheet>
    }
}
