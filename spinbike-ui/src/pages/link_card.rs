use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;

use crate::api;

#[derive(serde::Serialize)]
struct LinkReq {
    barcode: String,
}

#[derive(Clone, serde::Deserialize)]
#[allow(dead_code)]
struct CardResp {
    id: i64,
    barcode: String,
    credit: f64,
}

#[component]
pub fn LinkCardPage() -> impl IntoView {
    let barcode_ref = NodeRef::<leptos::html::Input>::new();
    let (error, set_error) = signal(String::new());
    let (success_msg, set_success_msg) = signal(String::new());
    let (loading, set_loading) = signal(false);

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        let barcode = barcode_ref
            .get()
            .map(|el| {
                let el: &HtmlInputElement = &el;
                el.value()
            })
            .unwrap_or_default();

        if barcode.is_empty() {
            return;
        }

        set_loading.set(true);
        set_error.set(String::new());
        set_success_msg.set(String::new());

        spawn_local(async move {
            match api::post::<LinkReq, CardResp>("/api/cards/link", &LinkReq { barcode }).await {
                Ok(card) => set_success_msg.set(format!(
                    "Card linked successfully! Barcode: {}, Credit: {:.0} CZK",
                    card.barcode, card.credit
                )),
                Err(e) => set_error.set(e),
            }
            set_loading.set(false);
        });
    };

    view! {
        <div style="max-width:400px;margin:0 auto">
            <h1 class="page-title">"Link Card"</h1>

            {move || {
                let e = error.get();
                if !e.is_empty() {
                    view! { <div class="alert alert-error">{e}</div> }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }
            }}

            {move || {
                let m = success_msg.get();
                if !m.is_empty() {
                    view! { <div class="alert alert-success">{m}</div> }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }
            }}

            <form on:submit=on_submit>
                <div class="form-group">
                    <label>"Card Barcode"</label>
                    <input type="text" class="form-control" node_ref=barcode_ref placeholder="Scan or enter barcode" required />
                </div>
                <button type="submit" class="btn btn-primary btn-block" disabled=move || loading.get()>
                    {move || if loading.get() { "Linking..." } else { "Link Card" }}
                </button>
            </form>
        </div>
    }
}
