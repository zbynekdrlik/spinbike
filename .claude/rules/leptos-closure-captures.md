---
paths:
  - "spinbike-ui/src/util.rs"
  - "spinbike-ui/src/pages/dashboard/action_form.rs"
  - "spinbike-ui/src/pages/reports/users_by_movement.rs"
---

# Leptos 0.8 CSR still needs `Send + Sync` closures, and shared non-Copy
# state needs clone-before-move at EVERY capture site (#344 wasm32 follow-up)

Two gotchas that only surface on `cargo check --target wasm32-unknown-unknown`
(the `error-codes.md` rule already documents a related "spinbike-ui isn't
covered by `cargo check --workspace`" gap — this is the same class of
"only CI catches it" problem, one level deeper).

## 1. Reactive dynamic-children closures need `Send + Sync`, even in CSR-only

Leptos 0.8's reactive dynamic-children machinery (the `.into_any()` /
`ReactiveFunction` path used by every `{move || ...}` view child and every
`on:click=` handler) requires the closures it stores to be `Send + Sync +
'static` — **even in this app, which is `csr`-only** (`spinbike-ui/Cargo.toml`:
`leptos = { features = ["csr"] }`, no `ssr`). The bound exists because
Leptos's closure-storage types are shared across the CSR/SSR/hydration code
paths, not because this app is actually multi-threaded (wasm32 is
single-threaded regardless).

**Consequence:** any `Rc<...>`/`Rc<Cell<...>>`/`RefCell`-backed helper type
that gets captured — even transitively, through another closure — by a
`{move || ...}` view child or an `on:click=` handler will fail wasm32 CI
with `E0277: ... cannot be sent between threads safely`, while compiling
fine for the native/server target (which never even builds this crate).
**Prefer `Arc<AtomicU32>` / `Arc<Mutex<...>>` / `Arc<RwLock<...>>` over
`Rc<Cell<...>>` / `Rc<RefCell<...>>` for any small shared-mutable-state
helper used from view code** — the atomic/Mutex forms are Send+Sync by
construction with no behavior change on a single-threaded wasm32 target,
and no extra runtime cost worth avoiding here.

Do NOT reach for a Leptos `StoredValue`/`ArcStoredValue` to fix this
instead — it reintroduces reactive-Owner/arena lifetime coupling, which is
exactly the "accessed a disposed reactive scope" WASM panic class this
codebase has hit before (`edit_info_form.rs`, #89) whenever a callback
outlives the component scope that created it (e.g. a 2.5s `spawn_local`
timer firing after the staff member switched customers, RequestId's whole
reason for existing per #344 finding 3). A plain `Arc<...>` has no such
coupling.

## 2. Clone-before-move needs its OWN scope per consumer — a chained shadow silently breaks

When one `Clone`-but-not-`Copy` value (a helper closure like
`schedule_msg_clear`, or a shared struct like `RequestId`) is used by
**more than one** top-level `move |..| {..}` closure, cloning it with a
same-scope repeated shadow does **NOT** work:

```rust
// WRONG — compiles for the FIRST consumer only.
let x = x.clone();
let do_topup = move |_ev| { ... x ... };   // moves the shadow above — OK

let x = x.clone();   // ERROR E0382: `x` was already moved into do_topup
let do_charge = move |_ev| { ... x ... };
```

`let x = x.clone();` at function scope creates a fresh binding — but
`move |..| {..}` on the very next line still moves that SAME binding
entirely into its own storage. The clone existed, but it got fully
consumed by the first consumer, so the following `x.clone()` line has
nothing left to clone from — you'll see `error[E0382]: use of moved value`
or `borrow of moved value` pointing at the SECOND (and any later) consumer.

**Fix: wrap each consumer in its own block, so each clone comes from the
untouched original:**

```rust
let do_topup = {
    let x = x.clone();
    move |_ev| { ... x ... }
};
let do_charge = {
    let x = x.clone();      // clones the ORIGINAL `x`, unaffected by do_topup's block-local shadow
    move |_ev| { ... x ... }
};
// the LAST consumer can skip the wrapping block and take the original directly —
// nothing needs `x` after it.
let last_consumer = move |_ev| { ... x ... };
```

This applies at **every level** a value crosses into a closure that must
outlive/repeat past that point:

- A value used by 2+ separate **top-level** `let name = move |..| {..};`
  bindings (e.g. `schedule_msg_clear` used by `do_topup`, `do_charge`, the
  Spinning quick-charge chip's `on_click`, AND `do_log_visit` in
  `action_form.rs`; `req_id` used by `Effect::new` and `on_show_more` in
  `users_by_movement.rs`) — wrap each closure literal in a block per above.
- A value used **inside** a closure that is itself called repeatedly
  (a click handler, or a reactive `{move || ...}` view child) AND that
  body constructs a nested `spawn_local(async move {...})` or another
  `move |..| {..}` — `async move`/`move` ALWAYS captures by full ownership
  transfer regardless of what the body needs, so without an inner
  `let x = x.clone();` right before the nested block, the OUTER closure
  can only be called ONCE (its first call moves `x` away) — this is the
  `error[E0525]: expected FnMut, found FnOnce` shape, with the compiler's
  own "closure is FnOnce because it moves the variable `x` out of its
  environment" pointing at exactly this pattern.
- A `Vec<View<...>>: Render` / `... to implement Render` error inside a
  `view! {}` block can be a downstream CASCADE of either of the above on
  one of the elements inside — fix the root FnMut/Send issue at the
  flagged inner closure first, then re-check whether the outer Render
  error is still present before treating it as a separate bug.

The compiler's own `help: consider cloning the value before moving it into
the closure` suggestion (E0382) shows the wrapping-block shape directly —
trust it over inventing a different restructuring.
