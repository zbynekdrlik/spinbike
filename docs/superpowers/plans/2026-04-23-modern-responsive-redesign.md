# SpinBike 2026 Modern Responsive Redesign — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Implement the 2026 modern responsive redesign across the whole app per `docs/superpowers/specs/2026-04-23-modern-responsive-redesign-design.md`.

**Architecture:** Adaptive dark/light design tokens + new primitive CSS classes (`.btn`, `.seg`, `.sheet`, `.list-row`, `.group`). Two new reusable Leptos components (Sheet, Segmented). Split dashboard.rs monolith into `pages/dashboard/*` module. Backend gains transaction pagination (`?limit`+`?before`). Keep every existing `data-testid` stable so existing E2E survives.

**Tech Stack:** Leptos 0.7 CSR + Axum 0.8 + SQLite + Trunk. Same stack, no framework change.

---

### Task 1: Bump VERSION

**Files:**
- Modify: `VERSION`
- Modify: `Cargo.toml` (workspace package list uses `version = "…"` in sub-crate manifests — run `scripts/sync-version.sh`)

- [ ] **Step 1:** Overwrite `VERSION` with `0.9.0`
- [ ] **Step 2:** Run `bash scripts/sync-version.sh` to propagate to all Cargo.toml files
- [ ] **Step 3:** `git diff` — verify only VERSION + Cargo.toml files changed
- [ ] **Step 4:** Commit:
  ```bash
  git add VERSION spinbike-ui/Cargo.toml crates/spinbike-core/Cargo.toml crates/spinbike-server/Cargo.toml Cargo.toml
  git commit -m "chore: bump VERSION to 0.9.0 for redesign"
  ```

---

### Task 2: style.css — adaptive tokens foundation

**Files:**
- Modify: `spinbike-ui/style.css` (the `:root { ... }` block only in this task)

- [ ] **Step 1:** Rewrite the `:root` token block with the full adaptive token set from the spec (spacing `--s-1..--s-7`, radius `--r-sm/--r/--r-lg/--r-pill`, font `--fs-xs..--fs-2xl`, touch targets `--tap-min/--tap-md/--tap-lg`, motion `--dur-*`, `--ease-*`, and all the dark palette colours).
- [ ] **Step 2:** Immediately after the dark `:root` block, add a light-theme override:
  ```css
  @media (prefers-color-scheme: light) {
      :root {
          --bg: #f6f7f9;
          --surface: #ffffff;
          --surface-2: #f1f3f7;
          --surface-3: #e5e8ef;
          --border: #dfe2ea;
          --border-strong: #c1c6d2;
          --text: #14161b;
          --text-muted: #545a67;
          --text-dim: #8a8f9b;
          --brand: #16a34a;
          --brand-tint: rgba(22, 163, 74, 0.10);
          --danger: #dc2626;
          --info: #2563eb;
          --pass: #65a30d;
          --shadow: 0 2px 8px rgba(13, 20, 45, 0.07);
          --shadow-lg: 0 16px 48px rgba(13, 20, 45, 0.12);
      }
  }
  ```
- [ ] **Step 3:** Bump `body { font-size }` to 16px explicitly.
- [ ] **Step 4:** `cargo fmt --all --check` — passes (CSS not touched by rustfmt but sanity-check no Rust was touched).
- [ ] **Step 5:** Commit: `feat(css): adaptive dark/light token foundation`.

---

### Task 3: style.css — rationalise `.btn`

**Files:**
- Modify: `spinbike-ui/style.css` (button section only)

- [ ] **Step 1:** Replace the current `.btn`, `.btn-sm`, `.btn-icon`, `.btn-primary`, `.btn-danger`, `.btn-outline`, `.btn-pass`, `.btn-block` with a canonical system:
  ```css
  .btn {
      display:inline-flex; align-items:center; justify-content:center; gap: var(--s-2);
      min-height: var(--tap-min); padding: 0 var(--s-4);
      font-size: var(--fs-sm); font-weight: 500; line-height: 1;
      border-radius: var(--r); border: 1px solid var(--border-strong);
      background: var(--surface-2); color: var(--text);
      cursor: pointer; font-family: inherit; text-decoration: none;
      transition: background var(--dur-fast) var(--ease-out), border-color var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out);
  }
  .btn:hover:not(:disabled) { background: var(--surface-3); border-color: var(--border-strong); }
  .btn:active:not(:disabled) { transform: translateY(1px); }
  .btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .btn--hero    { min-height: var(--tap-lg); padding: 0 var(--s-5); font-size: var(--fs-md); border-radius: var(--r-lg); }
  .btn--compact { min-height: 36px; padding: 0 var(--s-3); font-size: var(--fs-xs); border-radius: var(--r-sm); }
  .btn--block   { width: 100%; }

  .btn--primary { background: var(--brand); border-color: var(--brand); color: #0a1f10; font-weight: 600; }
  .btn--primary:hover:not(:disabled) { background: #1fb357; border-color: #1fb357; }

  .btn--pass    { background: var(--pass); border-color: var(--pass); color: #1a2906; font-weight: 600; }
  .btn--pass:hover:not(:disabled) { filter: brightness(1.08); }

  .btn--danger  { background: transparent; border-color: var(--danger); color: var(--danger); }
  .btn--danger:hover:not(:disabled) { background: var(--danger); color: #fff; }

  .btn--ghost   { background: transparent; border-color: var(--border); color: var(--text-muted); }
  .btn--ghost:hover:not(:disabled) { background: var(--surface-2); color: var(--text); }
  ```
- [ ] **Step 2:** Keep LEGACY aliases at the end of the block so existing markup keeps working until pages are migrated:
  ```css
  .btn-sm { /* DEPRECATED: alias for .btn.btn--compact */ min-height: 36px; padding: 0 var(--s-3); font-size: var(--fs-xs); border-radius: var(--r-sm); }
  .btn-icon { min-height: 36px; padding: 0 var(--s-3); font-size: var(--fs-xs); background: var(--surface-2); color: var(--text-muted); }
  .btn-primary { /* DEPRECATED: use .btn.btn--primary */ background: var(--brand); border-color: var(--brand); color: #0a1f10; font-weight: 600; }
  .btn-danger  { /* DEPRECATED: use .btn.btn--danger */ background: transparent; border-color: var(--danger); color: var(--danger); }
  .btn-outline { /* DEPRECATED: use .btn.btn--ghost */ background: transparent; border-color: var(--border); color: var(--text-muted); }
  .btn-pass    { /* DEPRECATED: use .btn.btn--pass */ background: var(--pass); border-color: var(--pass); color: #1a2906; font-weight: 600; }
  .btn-block   { width: 100%; }
  ```
  These aliases let per-page refactor tasks happen separately without breakage. Final cleanup task (Task 16) removes them.
- [ ] **Step 3:** Commit: `feat(css): rationalised .btn with BEM variants + legacy aliases`.

---

### Task 4: style.css — `.seg` segmented control + kill light-theme `.tabbar`

**Files:**
- Modify: `spinbike-ui/style.css`

- [ ] **Step 1:** Replace the `.tabbar / .tab / .tab--active / .tab-body` block (the light-theme clash) with:
  ```css
  .seg { display: flex; background: var(--surface-2); border-radius: var(--r); padding: 4px; gap: 2px; }
  .seg__item {
      flex: 1; min-height: 40px;
      background: transparent; color: var(--text-muted);
      border: 0; border-radius: calc(var(--r) - 4px);
      font-family: inherit; font-size: var(--fs-sm); font-weight: 500;
      cursor: pointer;
      transition: background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out);
  }
  .seg__item:hover { color: var(--text); }
  .seg__item[aria-selected="true"] {
      background: var(--surface);
      color: var(--text);
      box-shadow: var(--shadow);
  }
  .seg-body { padding-top: var(--s-3); }
  ```
- [ ] **Step 2:** Also remove `.txn-row--voided`'s light-theme colours (`#f5f5f5`, `#888`, `#b00020`), replace with:
  ```css
  .txn-row--voided       { color: var(--text-dim); opacity: 0.75; }
  .txn-row--voided .amount { text-decoration: line-through; }
  .txn-voided-tag        { font-size: var(--fs-xs); color: var(--danger); margin-left: 6px; text-transform: uppercase; letter-spacing: 0.04em; }
  ```
- [ ] **Step 3:** Keep `.tabs / .tab-btn` legacy block for now (also migrated to new tokens — `color: var(--text-muted)` etc.) until Task 16 cleanup.
- [ ] **Step 4:** Commit: `feat(css): .seg segmented control + remove clashing light-theme tabs/voided-row styles`.

---

### Task 5: style.css — `.sheet` + `.sheet-backdrop`

**Files:**
- Modify: `spinbike-ui/style.css`

- [ ] **Step 1:** Append (after the `.modal-overlay` block):
  ```css
  .sheet-backdrop {
      position: fixed; inset: 0;
      background: rgba(0, 0, 0, 0.45);
      -webkit-backdrop-filter: blur(4px);
      backdrop-filter: blur(4px);
      z-index: 200;
      animation: sheet-backdrop-in var(--dur) var(--ease-out);
  }
  .sheet {
      position: fixed; left: 0; right: 0; bottom: 0;
      background: var(--surface);
      border-top-left-radius: var(--r-lg);
      border-top-right-radius: var(--r-lg);
      padding: var(--s-3) var(--s-4) var(--s-5);
      z-index: 210;
      box-shadow: var(--shadow-lg);
      max-height: 90vh;
      overflow-y: auto;
      animation: sheet-in var(--dur-slow) var(--ease-spring);
  }
  .sheet__grab {
      width: 44px; height: 4px; border-radius: 2px;
      background: var(--border-strong);
      margin: 0 auto var(--s-3);
  }
  .sheet__title {
      font-size: var(--fs-md); font-weight: 600;
      margin-bottom: var(--s-4); letter-spacing: -0.005em;
  }
  .sheet__body > * + * { margin-top: var(--s-3); }
  .sheet__actions {
      display: flex; gap: var(--s-2);
      padding-top: var(--s-4); margin-top: var(--s-4);
      border-top: 1px solid var(--border);
  }
  .sheet__actions .btn { flex: 1; }

  @media (min-width: 768px) {
      .sheet {
          position: fixed;
          left: 50%; top: 50%; right: auto; bottom: auto;
          transform: translate(-50%, -50%);
          width: min(480px, 90vw);
          border-radius: var(--r-lg);
          animation: modal-in var(--dur-slow) var(--ease-spring);
      }
      .sheet__grab { display: none; }
  }

  @keyframes sheet-in { from { transform: translateY(100%); } to { transform: translateY(0); } }
  @keyframes modal-in { from { transform: translate(-50%, -45%); opacity: 0; } to { transform: translate(-50%, -50%); opacity: 1; } }
  @keyframes sheet-backdrop-in { from { opacity: 0; } to { opacity: 1; } }
  ```
- [ ] **Step 2:** Commit: `feat(css): .sheet bottom-sheet / desktop-modal primitive`.

---

### Task 6: style.css — `.group` + `.list-row`

**Files:**
- Modify: `spinbike-ui/style.css`

- [ ] **Step 1:** Append:
  ```css
  .group {
      background: var(--surface);
      border: 1px solid var(--border);
      border-radius: var(--r);
      overflow: hidden;
      margin-bottom: var(--s-4);
  }
  .group__head {
      padding: var(--s-2) var(--s-4);
      font-size: var(--fs-xs); font-weight: 600;
      color: var(--text-dim);
      text-transform: uppercase; letter-spacing: 0.05em;
      border-bottom: 1px solid var(--border);
      background: var(--surface-2);
  }
  .list-row {
      display: flex; align-items: center; gap: var(--s-3);
      padding: var(--s-3) var(--s-4);
      min-height: 56px;
      border-top: 1px solid var(--border);
  }
  .list-row:first-child { border-top: none; }
  .list-row--interactive { cursor: pointer; transition: background var(--dur-fast) var(--ease-out); }
  .list-row--interactive:hover { background: var(--surface-2); }
  .list-row__main { flex: 1; min-width: 0; }
  .list-row__title { font-weight: 500; color: var(--text); }
  .list-row__sub { font-size: var(--fs-sm); color: var(--text-muted); margin-top: 2px; }
  .list-row__amount { font-weight: 600; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .list-row__amount--pos { color: var(--brand); }
  .list-row__amount--neg { color: var(--danger); }
  .list-row__end { display: flex; align-items: center; gap: var(--s-2); flex-shrink: 0; }
  .list-row__accent {
      width: 3px; align-self: stretch; margin: -12px 0 -12px calc(-1 * var(--s-4));
      background: var(--border-strong);
  }
  .list-row__accent--available { background: var(--brand); }
  .list-row__accent--booked    { background: var(--info); }
  .list-row__accent--full      { background: var(--danger); }
  .list-row__accent--cancelled { background: var(--border-strong); opacity: 0.5; }
  ```
- [ ] **Step 2:** Commit: `feat(css): .group + .list-row unified list primitive`.

---

### Task 7: style.css — refresh badge / form / navbar / day-picker / pass-banner / alert / page

**Files:**
- Modify: `spinbike-ui/style.css`

- [ ] **Step 1:** Replace `.badge / .badge-booked / .badge-full / .badge-cancelled` with:
  ```css
  .badge {
      display: inline-flex; align-items: center;
      min-height: 22px; padding: 2px var(--s-2);
      border-radius: var(--r-pill);
      font-size: var(--fs-xs); font-weight: 600; letter-spacing: 0.02em;
      background: var(--surface-3); color: var(--text-muted);
      border: 1px solid transparent; white-space: nowrap;
  }
  .badge--pass      { background: rgba(132, 204, 22, 0.14); color: var(--pass); }
  .badge--booked    { background: rgba(96, 165, 250, 0.14); color: var(--info); }
  .badge--full      { background: rgba(248, 113, 113, 0.14); color: var(--danger); }
  .badge--cancelled { background: var(--surface-3); color: var(--text-dim); }
  .badge--voided    { background: rgba(248, 113, 113, 0.14); color: var(--danger); }
  ```
  Keep legacy `.badge-booked / .badge-full / .badge-cancelled` as aliases.
- [ ] **Step 2:** Bump `.form-control` to `min-height: var(--tap-min)` and `font-size: var(--fs-base)` (16 px — prevents iOS zoom on focus).
- [ ] **Step 3:** Update `.navbar` to use new tokens (replace hard-coded dark shadow with `var(--shadow)`, etc.). Keep sticky behaviour.
- [ ] **Step 4:** Bump `.day-btn` to `min-width: 64px; min-height: 72px; padding: var(--s-2)`; larger `.day-num`.
- [ ] **Step 5:** Update `.pass-banner`, `.pass-banner-ok`, `.pass-banner-expired` to use `--surface`, `--brand-tint`, `--danger` properly on both palettes.
- [ ] **Step 6:** Update `.alert-*` to use tokens.
- [ ] **Step 7:** Update `.page { max-width }` to `960px` (was 880 — wider gutter on desktop). Add `--page-pad: var(--s-4)` with `@media (min-width: 768px) { :root { --page-pad: var(--s-5); } }` and use it instead of hardcoded padding.
- [ ] **Step 8:** Commit: `feat(css): refresh badge/form/navbar/day-btn/pass-banner/alert on new tokens`.

---

### Task 8: Sheet component (reusable)

**Files:**
- Create: `spinbike-ui/src/components/sheet.rs`
- Modify: `spinbike-ui/src/components/mod.rs`

- [ ] **Step 1:** Create `sheet.rs`:
  ```rust
  //! Bottom-sheet primitive. On phone, slides up from the bottom.
  //! On desktop (>= 768px) CSS transforms it into a centered modal.
  //!
  //! Caller owns the `show` signal and passes children. Clicking the
  //! backdrop or pressing Escape closes via `on_close`.
  use leptos::ev;
  use leptos::prelude::*;
  use wasm_bindgen::JsCast;

  #[component]
  pub fn Sheet(
      /// Reactive open/closed flag.
      #[prop(into)] show: Signal<bool>,
      /// Callback invoked to request close (backdrop click / Esc).
      #[prop(into)] on_close: Callback<()>,
      /// Title shown at top of the sheet.
      #[prop(into)] title: String,
      /// Optional data-testid for E2E tests.
      #[prop(optional, into)] testid: Option<String>,
      children: ChildrenFn,
  ) -> impl IntoView {
      let close = move || on_close.run(());
      let on_backdrop = {
          let close = close.clone();
          move |_| close()
      };
      let on_escape = {
          let close = close.clone();
          move |e: ev::KeyboardEvent| if e.key() == "Escape" { close(); }
      };

      view! {
          <Show when=move || show.get()>
              <div class="sheet-backdrop" on:click=on_backdrop.clone() />
              <div
                  class="sheet"
                  role="dialog"
                  aria-modal="true"
                  tabindex="-1"
                  on:keydown=on_escape.clone()
                  data-testid=testid.clone().unwrap_or_default()
              >
                  <div class="sheet__grab" />
                  <div class="sheet__title">{title.clone()}</div>
                  <div class="sheet__body">{children()}</div>
              </div>
          </Show>
      }
  }
  ```
- [ ] **Step 2:** Add `pub mod sheet; pub use sheet::Sheet;` to `components/mod.rs`.
- [ ] **Step 3:** Commit: `feat(ui): Sheet component for mobile sheets / desktop modals`.

---

### Task 9: Segmented component (reusable)

**Files:**
- Create: `spinbike-ui/src/components/segmented.rs`
- Modify: `spinbike-ui/src/components/mod.rs`

- [ ] **Step 1:** Create `segmented.rs`:
  ```rust
  //! Segmented control primitive. Replaces the ad-hoc `.tabbar` + `.tabs`.
  //! Callers own an active `String` (or `&'static str`) signal.
  use leptos::prelude::*;

  #[component]
  pub fn Segmented(
      /// List of (key, label) pairs for the segment items.
      items: Vec<(String, String)>,
      /// Current active key.
      #[prop(into)] active: Signal<String>,
      /// Fired with the new key when a segment is tapped.
      #[prop(into)] on_change: Callback<String>,
      /// Optional prefix for the per-segment `data-testid` — `{prefix}-{key}`.
      #[prop(optional, into)] testid_prefix: Option<String>,
  ) -> impl IntoView {
      let items = StoredValue::new(items);
      let prefix = testid_prefix.unwrap_or_default();

      view! {
          <div class="seg" role="tablist">
              {move || {
                  items.with_value(|items| {
                      items.iter().map(|(k, label)| {
                          let key = k.clone();
                          let key_for_click = key.clone();
                          let key_for_sel = key.clone();
                          let testid = if prefix.is_empty() { String::new() } else { format!("{prefix}-{key}") };
                          view! {
                              <button
                                  class="seg__item"
                                  role="tab"
                                  aria-selected=move || if active.get() == key_for_sel { "true" } else { "false" }
                                  on:click=move |_| on_change.run(key_for_click.clone())
                                  data-testid=testid
                              >
                                  {label.clone()}
                              </button>
                          }
                      }).collect::<Vec<_>>()
                  })
              }}
          </div>
      }
  }
  ```
- [ ] **Step 2:** Add `pub mod segmented; pub use segmented::Segmented;` to `components/mod.rs`.
- [ ] **Step 3:** Commit: `feat(ui): Segmented control component`.

---

### Task 10: Backend — transaction pagination

**Files:**
- Modify: `crates/spinbike-server/src/db/transactions.rs`
- Modify: `crates/spinbike-server/src/routes/cards.rs`

- [ ] **Step 1:** Extend `list_by_card` signature with optional `limit` and `before` params. Append `AND created_at < ?` when `before` is Some, and `ORDER BY created_at DESC LIMIT ?` (default 10 when None, max 500).
- [ ] **Step 2:** In `routes/cards.rs`, accept `Query<TransactionsQuery { limit: Option<usize>, before: Option<String> }>` on `GET /api/cards/:id/transactions` and pipe through.
- [ ] **Step 3:** Add unit tests in `transactions.rs` for:
  - default limit returns at most 10 rows
  - `before=<ISO>` returns only older rows
  - explicit `limit` overrides default, capped at 500
- [ ] **Step 4:** Commit: `feat(server): transaction list pagination via limit+before`.

---

### Task 11: i18n — new keys

**Files:**
- Modify: `spinbike-ui/src/i18n.rs`

- [ ] **Step 1:** Insert before the closing `m` return:
  ```rust
  m.insert("show_older", ("Zobrazit starsie", "Show older"));
  m.insert("close", ("Zatvorit", "Close"));
  m.insert("edit_info", ("Upravit udaje", "Edit info"));
  m.insert("customer_info", ("Udaje klienta", "Customer info"));
  m.insert("sell_pass_label", ("Predat mesacny preukaz", "Sell monthly pass"));
  m.insert("pass_active_until", ("Aktivny do {}", "Active until {}"));
  m.insert("pass_expired_on", ("Skoncil {}", "Expired {}"));
  m.insert("days_left_short", ("{} d", "{}d"));
  m.insert("days_ago_short", ("pred {} d", "{}d ago"));
  ```
- [ ] **Step 2:** Commit: `feat(i18n): new keys for redesigned UI strings`.

---

### Task 12: Split dashboard.rs monolith

**Files:**
- Create: `spinbike-ui/src/pages/dashboard/mod.rs` (page entry)
- Create: `spinbike-ui/src/pages/dashboard/card_panel.rs`
- Create: `spinbike-ui/src/pages/dashboard/charge_section.rs`
- Create: `spinbike-ui/src/pages/dashboard/topup_section.rs`
- Create: `spinbike-ui/src/pages/dashboard/pass_banner.rs`
- Create: `spinbike-ui/src/pages/dashboard/transactions_list.rs`
- Create: `spinbike-ui/src/pages/dashboard/sheets/mod.rs`
- Create: `spinbike-ui/src/pages/dashboard/sheets/edit_info.rs`
- Create: `spinbike-ui/src/pages/dashboard/sheets/sell_pass.rs`
- Create: `spinbike-ui/src/pages/dashboard/sheets/edit_pass_date.rs`
- Delete: `spinbike-ui/src/pages/dashboard.rs`
- Modify: `spinbike-ui/src/pages/mod.rs`

- [ ] **Step 1:** Split the existing 1544-line `dashboard.rs` into the files above. Each sub-file exports one `#[component] pub fn`. Preserve every `data-testid` attribute exactly; preserve all current behaviour. This is a pure refactor — NO visual changes yet. Keep legacy class names in this step.
- [ ] **Step 2:** Update `pages/mod.rs`: change `pub mod dashboard;` to still work (the `mod.rs` replaces the file). Keep the `pub use dashboard::DashboardPage;` export.
- [ ] **Step 3:** Commit: `refactor(ui): split dashboard.rs into pages/dashboard/ module`.

---

### Task 13: Apply redesign to card detail panel + sheets

**Files:**
- Modify: `spinbike-ui/src/pages/dashboard/card_panel.rs`
- Modify: `spinbike-ui/src/pages/dashboard/pass_banner.rs`
- Modify: `spinbike-ui/src/pages/dashboard/charge_section.rs`
- Modify: `spinbike-ui/src/pages/dashboard/topup_section.rs`
- Modify: `spinbike-ui/src/pages/dashboard/transactions_list.rs`
- Modify: `spinbike-ui/src/pages/dashboard/sheets/edit_info.rs`
- Modify: `spinbike-ui/src/pages/dashboard/sheets/sell_pass.rs`
- Create: `spinbike-ui/src/pages/dashboard/sheets/edit_pass_date.rs`

- [ ] **Step 1 — card_panel.rs:** New layout per spec. Header bar at top, `.group` wrapping PassBanner + balance, primary actions row (`Charge`/`Topup` side-by-side via flex with gap), `Sell pass` as `.btn.btn--hero.btn--pass.btn--block`, secondary actions `Edit`/`Block` using `.btn.btn--ghost`, then `<Segmented items active on_change testid_prefix="tab" />` and content.
  - Replace the current `<div class="tabbar">…` with `<Segmented …>`.
  - Remove inline `style="…"` attributes — use classes. E.g. the `style="padding:2px 8px;font-size:0.85rem"` on the void button becomes `class="btn btn--compact btn--ghost"`.
  - Balance display becomes `<div class="card-balance"><span class="card-balance__num">42.50</span> <span class="card-balance__unit">€</span></div>`; add the supporting CSS:
    ```css
    .card-balance { display:flex; align-items:baseline; gap:4px; margin: var(--s-2) 0 var(--s-4); }
    .card-balance__num  { font-size: var(--fs-2xl); font-weight: 700; letter-spacing: -0.02em; font-variant-numeric: tabular-nums; }
    .card-balance__unit { font-size: var(--fs-lg); color: var(--text-muted); }
    .card-balance--negative .card-balance__num { color: var(--danger); }
    ```
- [ ] **Step 2 — pass_banner.rs:** Open as `.group`, convert "Edit date" to a button that flips a local `show_date_sheet` signal; render `<EditPassDateSheet … />`. Remove the inline date input — all date editing flows through the sheet.
- [ ] **Step 3 — charge_section.rs / topup_section.rs:** Keep inline (not sheets), but redo markup on `.group` + `.list-row` for the quick-amount chips row. Use `.btn.btn--compact` for chips, `.btn.btn--primary.btn--hero` for submit.
- [ ] **Step 4 — transactions_list.rs:**
  - New state: `limit: RwSignal<usize>` initialised to `10`.
  - Fetch uses `api::get::<Vec<TxnInfo>>(&format!("/api/cards/{card_id}/transactions?limit={}", limit.get()))`.
  - When `limit.get()` changes, effect re-fetches.
  - Render rows as `.list-row`s (not `<table>`) — date on left, action + service middle, amount right (class `.list-row__amount--neg` if negative, `--pos` if positive). Voided rows use `.list-row` + class `txn-row--voided`.
  - Void `×` button is `.btn.btn--compact.btn--ghost`.
  - Below the list: if `txns.len() >= limit.get()`, show a `.btn.btn--ghost.btn--block` labelled `{show_older}` that `set_limit.update(|n| *n += 20)`.
  - Empty state keeps current message in `.empty-state`.
- [ ] **Step 5 — sheets/edit_info.rs:** Wrap current `EditInfoForm` markup in `<Sheet show=show on_close=on_close title=i18n::t(lang.get(), "edit_info") testid="sheet-edit-info">…</Sheet>`. Move save/cancel to `.sheet__actions`. Remove the ad-hoc inline-expand path.
- [ ] **Step 6 — sheets/sell_pass.rs:** Wrap `SellPassModal` markup in `<Sheet title=i18n::t(lang.get(), "sell_pass_label") testid="sheet-sell-pass">…</Sheet>`. Remove the `.modal-overlay / .modal` markup.
- [ ] **Step 7 — sheets/edit_pass_date.rs (new):** A sheet component with a `<input type="date" class="form-control">`. PATCH `/api/transactions/{tx_id}/valid-until` on Save (same endpoint used today inside PassBanner).
- [ ] **Step 8:** Commit: `feat(ui): card detail panel on new design system + sheets`.

---

### Task 14: Apply redesign to schedule + class_card + day_picker

**Files:**
- Modify: `spinbike-ui/src/components/class_card.rs`
- Modify: `spinbike-ui/src/components/day_picker.rs`
- Modify: `spinbike-ui/src/pages/schedule.rs`
- Modify: `spinbike-ui/src/components/upcoming_classes.rs`
- Modify: `spinbike-ui/src/components/persistent_toggles.rs`

- [ ] **Step 1 — class_card.rs:** Replace `<div class="class-card available/booked/full/cancelled">` markup with `.list-row` + `.list-row__accent` with the right modifier. Instructor line becomes `.list-row__sub`, time becomes `.list-row__title`, action button in `.list-row__end` using `.btn.btn--primary` (Book) or `.btn.btn--danger.btn--compact` (Cancel).
- [ ] **Step 2 — day_picker.rs:** Markup already matches pattern. Just ensure class names are current.
- [ ] **Step 3 — schedule.rs:** Wrap class list in `.group`. Page title uses `.page-title`.
- [ ] **Step 4 — upcoming_classes.rs / persistent_toggles.rs:** Replace `<div class="upcoming-row"> / <div class="persistent-row">` grids with `.list-row`s. Ensure the "book" / "auto-cancel" / "cancel" / "toggle" buttons use `.btn.btn--primary` / `.btn.btn--danger` / `.btn.btn--ghost` appropriately. Keep every `data-testid` (`book-…`, `auto-cancel-…`, `persistent-toggle-…`).
- [ ] **Step 5:** Commit: `feat(ui): schedule + class cards + upcoming/persistent on new design system`.

---

### Task 15: Apply redesign to client pages, admin, nav

**Files:**
- Modify: `spinbike-ui/src/pages/my_balance.rs`
- Modify: `spinbike-ui/src/pages/my_bookings.rs`
- Modify: `spinbike-ui/src/pages/login.rs`
- Modify: `spinbike-ui/src/pages/link_card.rs`
- Modify: `spinbike-ui/src/pages/admin.rs`
- Modify: `spinbike-ui/src/pages/staff_dashboard.rs` (if distinct from dashboard/)
- Modify: `spinbike-ui/src/components/nav.rs`

- [ ] **Step 1 — my_balance.rs:** `.group` wrapping pass banner + balance + history. History uses `.list-row`s with the same "Show older" pattern as staff.
- [ ] **Step 2 — my_bookings.rs:** Upcoming bookings as `.list-row`s with cancel button `.btn.btn--danger.btn--compact`. Keep existing testids.
- [ ] **Step 3 — login.rs / link_card.rs:** Polished `.page-form` layout (max-width 420px, centered), `.form-control` bumped, primary button `.btn.btn--primary.btn--hero.btn--block`.
- [ ] **Step 4 — admin.rs:** Apply tokens to existing tables. `.data-table` uses `var(--surface-2)` head, `var(--surface)` body. Nav tabs restyled. No layout restructure — only token migration.
- [ ] **Step 5 — staff_dashboard.rs:** If this file is still in use, apply the same treatment as `dashboard/mod.rs` search bar and results list (`.form-control` search, `.group` of `.list-row` results).
- [ ] **Step 6 — nav.rs:** Keep existing structure; update class names to `.btn.btn--ghost.btn--compact` for nav buttons. Keep sticky.
- [ ] **Step 7:** Commit: `feat(ui): client pages + admin + nav on new design system`.

---

### Task 16: style.css — remove deprecated aliases

**Files:**
- Modify: `spinbike-ui/style.css`

- [ ] **Step 1:** Delete the `.btn-sm / .btn-icon / .btn-primary / .btn-danger / .btn-outline / .btn-pass / .btn-block (legacy) / .badge-booked / .badge-full / .badge-cancelled (legacy) / .tabs / .tab-btn / .modal-overlay / .modal / .upcoming-row / .persistent-row / .class-card` blocks — those classes are no longer referenced after Tasks 13–15.
  - Grep first: `rg '\.btn-sm|\.btn-icon|\.btn-primary|\.btn-danger|\.btn-outline|\.btn-pass|\.upcoming-row|\.persistent-row|\.class-card|\.tabbar|\.tab-btn|\.modal-overlay' spinbike-ui/src/`. Any remaining references MUST be fixed before deletion.
- [ ] **Step 2:** Commit: `refactor(css): remove deprecated aliases — single design system`.

---

### Task 17: Playwright — sheets, pagination, theme

**Files:**
- Create: `e2e/tests/redesign-sheets.spec.ts`
- Create: `e2e/tests/redesign-history-pagination.spec.ts`
- Create: `e2e/tests/redesign-theme.spec.ts`

- [ ] **Step 1 — redesign-sheets.spec.ts:** 3 tests —
  - Sell pass: open from card panel → `[data-testid="sheet-sell-pass"]` visible → cancel button → sheet gone → console clean.
  - Edit info: open → type name → Save → sheet closes → header reflects new name.
  - Escape key closes a sheet.
  - Backdrop click closes a sheet.
- [ ] **Step 2 — redesign-history-pagination.spec.ts:** 1 test —
  - Open a card that has many txns → count `.list-row` in history area → expect 10.
  - Click `[data-testid="show-older"]` → count 30.
  - Ensure console clean.
- [ ] **Step 3 — redesign-theme.spec.ts:** 1 test —
  - `page.emulateMedia({ colorScheme: 'light' })` → reload → assert `document.documentElement` computed `background-color` matches `rgb(246, 247, 249)` (the light `--bg`) — actually assert surface, since body uses `--bg` too — fetch via `getComputedStyle(document.body).backgroundColor` and match.
  - Switch to dark → similar assert with dark token value.
- [ ] **Step 4:** Commit: `test(e2e): sheet primitives, history pagination, adaptive theme`.

---

### Task 18: Fix missed migrations + final fmt + push

**Files:**
- As discovered

- [ ] **Step 1:** `cargo fmt --all --check` — must pass. Fix with `cargo fmt --all` if not.
- [ ] **Step 2:** Re-grep for any lingering deprecated class usage: `rg -n 'btn-sm|btn-icon|btn-primary|btn-danger|btn-outline|btn-pass|upcoming-row|persistent-row|class-card|tabbar|tab-btn|modal-overlay' spinbike-ui/src/`. Fix any strays.
- [ ] **Step 3:** `mkdir -p spinbike-ui/dist && echo "placeholder" > spinbike-ui/dist/index.html` — needed for `rust-embed` in lint job even though CI rebuilds it.
- [ ] **Step 4:** `git push origin dev` and monitor CI with `gh run view <id> --json status,conclusion,jobs` in background.

---

### Task 19: CI fixes (reactive)

Only if CI fails. Read `gh run view <id> --log-failed` and address the exact failure. No blind reruns.

---

### Task 20: PR create + wait for merge approval

- [ ] **Step 1:** `gh pr create --base main --head dev --title "feat: 2026 modern responsive redesign" --body <details>`.
- [ ] **Step 2:** `gh pr view <num> --json mergeable,mergeable_state` — must be `clean`.
- [ ] **Step 3:** Post PR URL to user with the completion report. **DO NOT MERGE** — wait for explicit user instruction.

---
