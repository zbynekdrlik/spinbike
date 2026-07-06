use leptos::prelude::*;

use crate::i18n::{self, Lang};

/// Build-time-injected version label. `CARGO_PKG_VERSION` resolves to the
/// `version` field in `spinbike-ui/Cargo.toml`, which is kept in sync with the
/// repo-wide `VERSION` file (see `scripts/sync-version.sh`). The server's
/// `/api/version` endpoint reads its own `CARGO_PKG_VERSION` from the same
/// source, so the two must agree on every build.
pub const VERSION_STRING: &str = env!("CARGO_PKG_VERSION");

/// Sticky bottom-right version label, rendered once at the App level so it
/// shows on every route. Used by post-deploy verification: a Playwright probe
/// reads the DOM string and compares against `/api/version`.
#[component]
pub fn VersionFooter() -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");
    view! {
        <div
            class="app-version"
            data-testid="version"
            aria-label=move || i18n::t(lang.get(), "version_footer_aria")
        >
            "v"{VERSION_STRING}
        </div>
    }
}
