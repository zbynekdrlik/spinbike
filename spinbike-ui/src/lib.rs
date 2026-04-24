pub mod api;
pub mod auth;
pub mod components;
pub mod i18n;
pub mod pages;
pub mod router;
pub mod util;
pub mod ws;

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(router::App);
}
