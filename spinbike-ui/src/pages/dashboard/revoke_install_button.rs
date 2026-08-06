//! "Odhlasit zariadenia" admin action (#261, round-5 belt-and-braces) —
//! revokes every LIVE `purpose='install'` token for a user, forcing any
//! installed home-screen app to fall back to the in-app email-code login on
//! its next open. A solo-operator desk action: there is no per-device list,
//! this simply cuts every install credential the user currently holds
//! (`POST /api/users/{id}/revoke-install-tokens`, staff-only).

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api;
use crate::i18n::{self, Lang};

#[derive(serde::Deserialize)]
struct RevokeInstallTokensResp {
    revoked: u64,
}

#[component]
pub fn RevokeInstallTokensButton(
    card_id: i64,
    set_msg: WriteSignal<String>,
    /// Red-alert channel (#126) — a failed revoke renders here, not in the
    /// green success alert. Same pattern as `BlockButton`.
    set_err: WriteSignal<String>,
) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    let (loading, set_loading) = signal(false);

    let on_click = move |_| {
        // Clear any stale alert from a previous action before starting a new
        // one (same discipline as BlockButton — see its own comment).
        set_msg.set(String::new());
        set_err.set(String::new());
        set_loading.set(true);
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req {}
            match api::post::<Req, RevokeInstallTokensResp>(
                &format!("/api/users/{card_id}/revoke-install-tokens"),
                &Req {},
            )
            .await
            {
                Ok(r) => {
                    set_msg.set(i18n::tf(
                        lang.get_untracked(),
                        "revoke_install_tokens_ok",
                        &[&r.revoked.to_string()],
                    ));
                }
                Err(e) => set_err.set(i18n::tf(lang.get_untracked(), "error_format", &[&e])),
            }
            set_loading.set(false);
        });
    };

    view! {
        <button
            class="btn btn--ghost btn--compact"
            data-testid="revoke-install-tokens-button"
            disabled=move || loading.get()
            on:click=on_click
        >
            {move || i18n::t(lang.get(), "revoke_install_tokens")}
        </button>
    }
}
