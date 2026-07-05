pub mod api;
pub mod auth;
pub mod components;
pub mod i18n;
pub mod pages;
pub mod relative_date;
pub mod router;
pub mod util;
pub mod ws;

// Gate behind cfg(not(test)) so wasm-pack test --node doesn't see two
// entry symbols named `main` (the test harness generates its own). The
// import is gated too — under cfg(test) nothing below uses it.
#[cfg(not(test))]
use wasm_bindgen::prelude::*;

#[cfg(not(test))]
#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(router::App);
}
