use leptos::prelude::*;

/// iOS-style segmented control used as a tab switcher.
///
/// Renders `<div class="seg" role="tablist">` containing one `<button class="seg__item"
/// role="tab">` per item. The active button carries `aria-selected="true"`.
///
/// `items` is `Vec<(key, label)>`. Clicking a button fires `on_change` with the
/// corresponding key. `testid_prefix`, when supplied, produces
/// `data-testid="{prefix}-{key}"` on each button for stable Playwright selectors.
#[component]
pub fn Segmented(
    /// `(key, label)` pairs defining the tabs.
    items: Vec<(String, String)>,
    /// The currently active key.
    #[prop(into)]
    active: Signal<String>,
    /// Called with the key when the user selects a different tab.
    #[prop(into)]
    on_change: Callback<String>,
    /// If present, each button gets `data-testid="{testid_prefix}-{key}"`.
    #[prop(optional, into)]
    testid_prefix: Option<String>,
) -> impl IntoView {
    let prefix = testid_prefix.unwrap_or_default();

    let buttons: Vec<_> = items
        .into_iter()
        .map(|(key, label)| {
            // Pre-compute the per-button data-testid once at render time;
            // empty string when no prefix is configured (Leptos drops empty
            // attributes), avoiding the closure-returning-Option pattern.
            let testid = if prefix.is_empty() {
                String::new()
            } else {
                format!("{prefix}-{key}")
            };
            let key_stored = StoredValue::new(key);
            let label_stored = StoredValue::new(label);

            view! {
                <button
                    class="seg__item"
                    role="tab"
                    aria-selected=move || {
                        if active.get() == key_stored.get_value() { "true" } else { "false" }
                    }
                    data-testid=testid
                    on:click=move |_| on_change.run(key_stored.get_value())
                >
                    {move || label_stored.get_value()}
                </button>
            }
        })
        .collect();

    view! {
        <div class="seg" role="tablist">
            {buttons}
        </div>
    }
}
