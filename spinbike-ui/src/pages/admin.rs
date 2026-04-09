use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_sys::{HtmlInputElement, HtmlSelectElement};

use crate::api;

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
struct TemplateRow {
    id: i64,
    weekday: i64,
    start_time: String,
    duration_minutes: i64,
    instructor_id: Option<i64>,
    capacity: i64,
    active: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct InstructorRow {
    id: i64,
    name: String,
    active: i64,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ServiceRow {
    id: i64,
    name: String,
    default_price: f64,
    active: i64,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct UserRow {
    id: i64,
    email: String,
    name: String,
    role: String,
    created_at: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct SettingRow {
    key: String,
    value: String,
}

static WEEKDAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

#[component]
pub fn AdminPage() -> impl IntoView {
    let (tab, set_tab) = signal("templates".to_string());

    let tabs = ["templates", "instructors", "services", "users", "settings"];
    let tab_buttons: Vec<_> = tabs.iter().map(|t| {
        let t = t.to_string();
        let t2 = t.clone();
        let t3 = t.clone();
        view! {
            <button
                class=move || if tab.get() == t { "tab-btn active" } else { "tab-btn" }
                on:click=move |_| set_tab.set(t2.clone())
            >
                {t3}
            </button>
        }
    }).collect();

    view! {
        <h1 class="page-title">"Admin"</h1>
        <div class="tabs">{tab_buttons}</div>
        {move || {
            match tab.get().as_str() {
                "templates" => TemplatesTab().into_any(),
                "instructors" => InstructorsTab().into_any(),
                "services" => ServicesTab().into_any(),
                "users" => UsersTab().into_any(),
                "settings" => SettingsTab().into_any(),
                _ => view! { <p>"Unknown"</p> }.into_any(),
            }
        }}
    }
}

// ---- Templates Tab ----

#[component]
fn TemplatesTab() -> impl IntoView {
    let (items, set_items) = signal(Vec::<TemplateRow>::new());
    let (loading, set_loading) = signal(true);
    let (ver, set_ver) = signal(0u32);
    let (msg, set_msg) = signal(String::new());

    Effect::new(move || {
        let _ = ver.get();
        set_loading.set(true);
        spawn_local(async move {
            match api::get::<Vec<TemplateRow>>("/api/admin/templates").await {
                Ok(d) => set_items.set(d),
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
            set_loading.set(false);
        });
    });

    let weekday_ref = NodeRef::<leptos::html::Select>::new();
    let time_ref = NodeRef::<leptos::html::Input>::new();
    let dur_ref = NodeRef::<leptos::html::Input>::new();
    let cap_ref = NodeRef::<leptos::html::Input>::new();
    let instr_ref = NodeRef::<leptos::html::Input>::new();

    let on_create = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        let weekday: i64 = weekday_ref.get().map(|el| { let el: &HtmlSelectElement = &el; el.value() }).unwrap_or_default().parse().unwrap_or(0);
        let start_time = time_ref.get().map(|el| { let el: &HtmlInputElement = &el; el.value() }).unwrap_or_default();
        let duration: i64 = dur_ref.get().map(|el| { let el: &HtmlInputElement = &el; el.value() }).unwrap_or_default().parse().unwrap_or(45);
        let capacity: i64 = cap_ref.get().map(|el| { let el: &HtmlInputElement = &el; el.value() }).unwrap_or_default().parse().unwrap_or(20);
        let instructor_id: Option<i64> = instr_ref.get().and_then(|el| { let el: &HtmlInputElement = &el; el.value().parse().ok() });

        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req { weekday: i64, start_time: String, duration_minutes: i64, instructor_id: Option<i64>, capacity: i64 }
            match api::post::<Req, TemplateRow>("/api/admin/templates", &Req { weekday, start_time, duration_minutes: duration, instructor_id, capacity }).await {
                Ok(_) => set_ver.update(|v| *v += 1),
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
        });
    };

    let weekday_options: Vec<_> = (0..7).map(|i: usize| {
        let val = i.to_string();
        let name = WEEKDAY_NAMES[i];
        view! { <option value=val>{name}</option> }
    }).collect();

    view! {
        {move || { let m = msg.get(); if m.is_empty() { view! { <span></span> }.into_any() } else { view! { <div class="alert alert-info">{m}</div> }.into_any() } }}

        <form class="inline-form mb-2" on:submit=on_create>
            <div class="form-group">
                <label>"Weekday"</label>
                <select class="form-control" node_ref=weekday_ref>{weekday_options}</select>
            </div>
            <div class="form-group">
                <label>"Start Time"</label>
                <input type="time" class="form-control" node_ref=time_ref required />
            </div>
            <div class="form-group">
                <label>"Duration"</label>
                <input type="number" class="form-control" node_ref=dur_ref value="45" min="15" step="5" />
            </div>
            <div class="form-group">
                <label>"Capacity"</label>
                <input type="number" class="form-control" node_ref=cap_ref value="20" min="1" />
            </div>
            <div class="form-group">
                <label>"Instructor ID"</label>
                <input type="number" class="form-control" node_ref=instr_ref placeholder="Optional" />
            </div>
            <button type="submit" class="btn btn-primary btn-sm">"Create"</button>
        </form>

        {move || {
            if loading.get() {
                return view! { <div class="text-center"><span class="spinner"></span></div> }.into_any();
            }
            let list = items.get();
            let rows: Vec<_> = list.iter().map(|t| {
                let tid = t.id;
                let set_v = set_ver;
                let set_m = set_msg;
                let day_name = WEEKDAY_NAMES.get(t.weekday as usize).unwrap_or(&"?").to_string();
                let time = t.start_time.clone();
                let dur = format!("{}m", t.duration_minutes);
                let cap = t.capacity;
                let instr = t.instructor_id.map(|i| i.to_string()).unwrap_or_else(|| "-".into());
                let (editing, set_editing) = signal(false);
                let edit_cap_ref = NodeRef::<leptos::html::Input>::new();
                let edit_time_ref = NodeRef::<leptos::html::Input>::new();
                let time_for_edit = t.start_time.clone();
                let cap_for_edit = t.capacity;
                let on_del = move |_| {
                    spawn_local(async move {
                        let _ = api::delete(&format!("/api/admin/templates/{tid}")).await;
                        set_v.update(|v| *v += 1);
                    });
                };
                let on_save = move |_| {
                    let new_time = edit_time_ref.get().map(|el| { let el: &HtmlInputElement = &el; el.value() }).unwrap_or_default();
                    let new_cap: i64 = edit_cap_ref.get().map(|el| { let el: &HtmlInputElement = &el; el.value() }).unwrap_or_default().parse().unwrap_or(0);
                    spawn_local(async move {
                        #[derive(serde::Serialize)]
                        struct Req { start_time: Option<String>, capacity: Option<i64> }
                        match api::put(&format!("/api/admin/templates/{tid}"), &Req { start_time: Some(new_time), capacity: Some(new_cap) }).await {
                            Ok(_) => { set_editing.set(false); set_v.update(|v| *v += 1); }
                            Err(e) => set_m.set(format!("Error: {e}")),
                        }
                    });
                };
                view! {
                    <tr>
                        <td>{tid}</td>
                        <td>{day_name}</td>
                        <td>{time}</td>
                        <td>{dur}</td>
                        <td>{cap}</td>
                        <td>{instr}</td>
                    </tr>
                    <tr>
                        <td colspan="6">
                            <button class="btn btn-sm btn-danger" on:click=on_del>"Delete"</button>
                            <button class="btn btn-sm btn-outline" style="margin-left:4px" on:click=move |_| set_editing.update(|v| *v = !*v)>"Edit"</button>
                        </td>
                    </tr>
                    {move || {
                        if editing.get() {
                            let time_val = time_for_edit.clone();
                            view! {
                                <tr>
                                    <td colspan="6">
                                        <div class="inline-form" style="display:flex;gap:8px;align-items:center;padding:4px 0">
                                            <label>"Time"</label>
                                            <input type="time" class="form-control" style="width:auto" node_ref=edit_time_ref value=time_val />
                                            <label>"Capacity"</label>
                                            <input type="number" class="form-control" style="width:80px" node_ref=edit_cap_ref value=cap_for_edit min="1" />
                                            <button class="btn btn-sm btn-primary" on:click=on_save>"Save"</button>
                                        </div>
                                    </td>
                                </tr>
                            }.into_any()
                        } else {
                            view! { <tr style="display:none"></tr> }.into_any()
                        }
                    }}
                }.into_any()
            }).collect();
            view! {
                <table>
                    <tbody>{rows}</tbody>
                </table>
            }.into_any()
        }}
    }
}

// ---- Instructors Tab ----

#[component]
fn InstructorsTab() -> impl IntoView {
    let (items, set_items) = signal(Vec::<InstructorRow>::new());
    let (loading, set_loading) = signal(true);
    let (ver, set_ver) = signal(0u32);
    let (msg, set_msg) = signal(String::new());
    let name_ref = NodeRef::<leptos::html::Input>::new();

    Effect::new(move || {
        let _ = ver.get();
        set_loading.set(true);
        spawn_local(async move {
            match api::get::<Vec<InstructorRow>>("/api/admin/instructors").await {
                Ok(d) => set_items.set(d),
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
            set_loading.set(false);
        });
    });

    let on_create = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        let name = name_ref.get().map(|el| { let el: &HtmlInputElement = &el; el.value() }).unwrap_or_default();
        if name.is_empty() { return; }
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req { name: String }
            match api::post::<Req, InstructorRow>("/api/admin/instructors", &Req { name }).await {
                Ok(_) => set_ver.update(|v| *v += 1),
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
        });
    };

    view! {
        {move || { let m = msg.get(); if m.is_empty() { view! { <span></span> }.into_any() } else { view! { <div class="alert alert-info">{m}</div> }.into_any() } }}
        <form class="inline-form mb-2" on:submit=on_create>
            <div class="form-group">
                <label>"Name"</label>
                <input type="text" class="form-control" node_ref=name_ref required />
            </div>
            <button type="submit" class="btn btn-primary btn-sm">"Add Instructor"</button>
        </form>

        {move || {
            if loading.get() {
                return view! { <div class="text-center"><span class="spinner"></span></div> }.into_any();
            }
            let list = items.get();
            let rows: Vec<_> = list.iter().map(|i| {
                let iid = i.id;
                let name = i.name.clone();
                let is_active = i.active != 0;
                let active_label = if is_active { "Active" } else { "Inactive" };
                let toggle_label = if is_active { "Deactivate" } else { "Activate" };
                let set_v = set_ver;
                let set_m = set_msg;
                let (editing, set_editing) = signal(false);
                let edit_name_ref = NodeRef::<leptos::html::Input>::new();
                let name_for_edit = i.name.clone();
                let on_toggle = move |_| {
                    let new_active = !is_active;
                    spawn_local(async move {
                        #[derive(serde::Serialize)]
                        struct Req { active: Option<bool> }
                        match api::put(&format!("/api/admin/instructors/{iid}"), &Req { active: Some(new_active) }).await {
                            Ok(_) => set_v.update(|v| *v += 1),
                            Err(e) => set_m.set(format!("Error: {e}")),
                        }
                    });
                };
                let on_save = move |_| {
                    let new_name = edit_name_ref.get().map(|el| { let el: &HtmlInputElement = &el; el.value() }).unwrap_or_default();
                    spawn_local(async move {
                        #[derive(serde::Serialize)]
                        struct Req { name: Option<String> }
                        match api::put(&format!("/api/admin/instructors/{iid}"), &Req { name: Some(new_name) }).await {
                            Ok(_) => { set_editing.set(false); set_v.update(|v| *v += 1); }
                            Err(e) => set_m.set(format!("Error: {e}")),
                        }
                    });
                };
                view! {
                    <tr>
                        <td>{iid}</td>
                        <td>{name}</td>
                        <td>{active_label}</td>
                        <td>
                            <button class="btn btn-sm btn-outline" on:click=on_toggle>{toggle_label}</button>
                            <button class="btn btn-sm btn-outline" style="margin-left:4px" on:click=move |_| set_editing.update(|v| *v = !*v)>"Edit"</button>
                        </td>
                    </tr>
                    {move || {
                        if editing.get() {
                            let nval = name_for_edit.clone();
                            view! {
                                <tr>
                                    <td colspan="4">
                                        <div class="inline-form" style="display:flex;gap:8px;align-items:center;padding:4px 0">
                                            <label>"Name"</label>
                                            <input type="text" class="form-control" style="width:auto" node_ref=edit_name_ref value=nval />
                                            <button class="btn btn-sm btn-primary" on:click=on_save>"Save"</button>
                                        </div>
                                    </td>
                                </tr>
                            }.into_any()
                        } else {
                            view! { <tr style="display:none"></tr> }.into_any()
                        }
                    }}
                }.into_any()
            }).collect();
            view! {
                <table>
                    <tbody>{rows}</tbody>
                </table>
            }.into_any()
        }}
    }
}

// ---- Services Tab ----

#[component]
fn ServicesTab() -> impl IntoView {
    let (items, set_items) = signal(Vec::<ServiceRow>::new());
    let (loading, set_loading) = signal(true);
    let (ver, set_ver) = signal(0u32);
    let (msg, set_msg) = signal(String::new());
    let name_ref = NodeRef::<leptos::html::Input>::new();
    let price_ref = NodeRef::<leptos::html::Input>::new();

    Effect::new(move || {
        let _ = ver.get();
        set_loading.set(true);
        spawn_local(async move {
            match api::get::<Vec<ServiceRow>>("/api/admin/services").await {
                Ok(d) => set_items.set(d),
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
            set_loading.set(false);
        });
    });

    let on_create = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        let name = name_ref.get().map(|el| { let el: &HtmlInputElement = &el; el.value() }).unwrap_or_default();
        let price: f64 = price_ref.get().map(|el| { let el: &HtmlInputElement = &el; el.value() }).unwrap_or_default().parse().unwrap_or(0.0);
        if name.is_empty() { return; }
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req { name: String, default_price: f64 }
            match api::post::<Req, ServiceRow>("/api/admin/services", &Req { name, default_price: price }).await {
                Ok(_) => set_ver.update(|v| *v += 1),
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
        });
    };

    view! {
        {move || { let m = msg.get(); if m.is_empty() { view! { <span></span> }.into_any() } else { view! { <div class="alert alert-info">{m}</div> }.into_any() } }}
        <form class="inline-form mb-2" on:submit=on_create>
            <div class="form-group">
                <label>"Name"</label>
                <input type="text" class="form-control" node_ref=name_ref required />
            </div>
            <div class="form-group">
                <label>"Price (CZK)"</label>
                <input type="number" class="form-control" node_ref=price_ref step="1" min="0" required />
            </div>
            <button type="submit" class="btn btn-primary btn-sm">"Add Service"</button>
        </form>

        {move || {
            if loading.get() {
                return view! { <div class="text-center"><span class="spinner"></span></div> }.into_any();
            }
            let list = items.get();
            let rows: Vec<_> = list.iter().map(|s| {
                let sid = s.id;
                let name = s.name.clone();
                let price = format!("{:.0}", s.default_price);
                let is_active = s.active != 0;
                let active_label = if is_active { "Active" } else { "Inactive" };
                let toggle_label = if is_active { "Deactivate" } else { "Activate" };
                let set_v = set_ver;
                let set_m = set_msg;
                let (editing, set_editing) = signal(false);
                let edit_name_ref = NodeRef::<leptos::html::Input>::new();
                let edit_price_ref = NodeRef::<leptos::html::Input>::new();
                let name_for_edit = s.name.clone();
                let price_for_edit = s.default_price;
                let on_toggle = move |_| {
                    let new_active = !is_active;
                    spawn_local(async move {
                        #[derive(serde::Serialize)]
                        struct Req { active: Option<bool> }
                        match api::put(&format!("/api/admin/services/{sid}"), &Req { active: Some(new_active) }).await {
                            Ok(_) => set_v.update(|v| *v += 1),
                            Err(e) => set_m.set(format!("Error: {e}")),
                        }
                    });
                };
                let on_save = move |_| {
                    let new_name = edit_name_ref.get().map(|el| { let el: &HtmlInputElement = &el; el.value() }).unwrap_or_default();
                    let new_price: f64 = edit_price_ref.get().map(|el| { let el: &HtmlInputElement = &el; el.value() }).unwrap_or_default().parse().unwrap_or(0.0);
                    spawn_local(async move {
                        #[derive(serde::Serialize)]
                        struct Req { name: Option<String>, default_price: Option<f64> }
                        match api::put(&format!("/api/admin/services/{sid}"), &Req { name: Some(new_name), default_price: Some(new_price) }).await {
                            Ok(_) => { set_editing.set(false); set_v.update(|v| *v += 1); }
                            Err(e) => set_m.set(format!("Error: {e}")),
                        }
                    });
                };
                view! {
                    <tr>
                        <td>{sid}</td>
                        <td>{name}</td>
                        <td>{price}</td>
                        <td>{active_label}</td>
                        <td>
                            <button class="btn btn-sm btn-outline" on:click=on_toggle>{toggle_label}</button>
                            <button class="btn btn-sm btn-outline" style="margin-left:4px" on:click=move |_| set_editing.update(|v| *v = !*v)>"Edit"</button>
                        </td>
                    </tr>
                    {move || {
                        if editing.get() {
                            let nval = name_for_edit.clone();
                            view! {
                                <tr>
                                    <td colspan="5">
                                        <div class="inline-form" style="display:flex;gap:8px;align-items:center;padding:4px 0">
                                            <label>"Name"</label>
                                            <input type="text" class="form-control" style="width:auto" node_ref=edit_name_ref value=nval />
                                            <label>"Price"</label>
                                            <input type="number" class="form-control" style="width:80px" node_ref=edit_price_ref value=price_for_edit step="1" min="0" />
                                            <button class="btn btn-sm btn-primary" on:click=on_save>"Save"</button>
                                        </div>
                                    </td>
                                </tr>
                            }.into_any()
                        } else {
                            view! { <tr style="display:none"></tr> }.into_any()
                        }
                    }}
                }.into_any()
            }).collect();
            view! {
                <table>
                    <tbody>{rows}</tbody>
                </table>
            }.into_any()
        }}
    }
}

// ---- Users Tab ----

#[component]
fn UsersTab() -> impl IntoView {
    let (items, set_items) = signal(Vec::<UserRow>::new());
    let (loading, set_loading) = signal(true);
    let (ver, set_ver) = signal(0u32);
    let (msg, set_msg) = signal(String::new());

    Effect::new(move || {
        let _ = ver.get();
        set_loading.set(true);
        spawn_local(async move {
            match api::get::<Vec<UserRow>>("/api/admin/users").await {
                Ok(d) => set_items.set(d),
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
            set_loading.set(false);
        });
    });

    view! {
        {move || { let m = msg.get(); if m.is_empty() { view! { <span></span> }.into_any() } else { view! { <div class="alert alert-info">{m}</div> }.into_any() } }}

        {move || {
            if loading.get() {
                return view! { <div class="text-center"><span class="spinner"></span></div> }.into_any();
            }
            let list = items.get();
            let rows: Vec<_> = list.iter().map(|u| {
                let uid = u.id;
                let email = u.email.clone();
                let name = u.name.clone();
                let current_role = u.role.clone();
                let created = u.created_at.clone();
                let set_v = set_ver;
                let set_m = set_msg;
                let role_ref = NodeRef::<leptos::html::Select>::new();

                let on_change_role = move |_| {
                    let role = role_ref.get().map(|el| { let el: &HtmlSelectElement = &el; el.value() }).unwrap_or_default();
                    spawn_local(async move {
                        #[derive(serde::Serialize)]
                        struct Req { role: String }
                        match api::put(&format!("/api/admin/users/{uid}/role"), &Req { role: role.clone() }).await {
                            Ok(_) => { set_m.set(format!("User {uid} role updated")); set_v.update(|v| *v += 1); }
                            Err(e) => set_m.set(format!("Error: {e}")),
                        }
                    });
                };

                let role_options: Vec<_> = ["customer", "staff", "admin"].iter().map(|r| {
                    let selected = *r == current_role.as_str();
                    let val = r.to_string();
                    let label = r.to_string();
                    if selected {
                        view! { <option value=val selected>{label}</option> }.into_any()
                    } else {
                        view! { <option value=val>{label}</option> }.into_any()
                    }
                }).collect();

                view! {
                    <div class="card mb-1">
                        <div class="flex justify-between items-center">
                            <div>
                                <strong>{name}</strong>
                                <span class="text-muted">{format!(" ({email})")}</span>
                            </div>
                            <select class="form-control" style="width:auto;display:inline" node_ref=role_ref on:change=on_change_role>
                                {role_options}
                            </select>
                        </div>
                        <div class="text-muted" style="font-size:0.75rem">{format!("ID: {uid} | Created: {created}")}</div>
                    </div>
                }.into_any()
            }).collect();
            view! { <div>{rows}</div> }.into_any()
        }}
    }
}

// ---- Settings Tab ----

#[component]
fn SettingsTab() -> impl IntoView {
    let (items, set_items) = signal(Vec::<SettingRow>::new());
    let (loading, set_loading) = signal(true);
    let (ver, set_ver) = signal(0u32);
    let (msg, set_msg) = signal(String::new());
    let key_ref = NodeRef::<leptos::html::Input>::new();
    let val_ref = NodeRef::<leptos::html::Input>::new();

    Effect::new(move || {
        let _ = ver.get();
        set_loading.set(true);
        spawn_local(async move {
            match api::get::<Vec<SettingRow>>("/api/admin/settings").await {
                Ok(d) => set_items.set(d),
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
            set_loading.set(false);
        });
    });

    let on_save = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();
        let key = key_ref.get().map(|el| { let el: &HtmlInputElement = &el; el.value() }).unwrap_or_default();
        let value = val_ref.get().map(|el| { let el: &HtmlInputElement = &el; el.value() }).unwrap_or_default();
        if key.is_empty() { return; }
        spawn_local(async move {
            #[derive(serde::Serialize)]
            struct Req { key: String, value: String }
            match api::put("/api/admin/settings", &Req { key, value }).await {
                Ok(_) => set_ver.update(|v| *v += 1),
                Err(e) => set_msg.set(format!("Error: {e}")),
            }
        });
    };

    view! {
        {move || { let m = msg.get(); if m.is_empty() { view! { <span></span> }.into_any() } else { view! { <div class="alert alert-info">{m}</div> }.into_any() } }}
        <form class="inline-form mb-2" on:submit=on_save>
            <div class="form-group">
                <label>"Key"</label>
                <input type="text" class="form-control" node_ref=key_ref required />
            </div>
            <div class="form-group">
                <label>"Value"</label>
                <input type="text" class="form-control" node_ref=val_ref required />
            </div>
            <button type="submit" class="btn btn-primary btn-sm">"Save"</button>
        </form>

        {move || {
            if loading.get() {
                return view! { <div class="text-center"><span class="spinner"></span></div> }.into_any();
            }
            let list = items.get();
            let rows: Vec<_> = list.iter().map(|s| {
                let key = s.key.clone();
                let value = s.value.clone();
                view! { <tr><td>{key}</td><td>{value}</td></tr> }
            }).collect();
            view! {
                <table>
                    <tbody>{rows}</tbody>
                </table>
            }.into_any()
        }}
    }
}
