use leptos::prelude::*;

static DAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

#[component]
pub fn DayPicker(
    /// (year, month, day) tuples for Mon-Sun of current week
    days: Vec<(i32, u32, u32)>,
    selected_idx: ReadSignal<usize>,
    set_selected_idx: WriteSignal<usize>,
) -> impl IntoView {
    view! {
        <div class="day-picker">
            {days.into_iter().enumerate().map(|(i, (_y, _m, d))| {
                let name = DAY_NAMES[i];
                view! {
                    <button
                        class=move || if selected_idx.get() == i { "day-btn active" } else { "day-btn" }
                        on:click=move |_| set_selected_idx.set(i)
                    >
                        <span class="day-name">{name}</span>
                        <span class="day-num">{d}</span>
                    </button>
                }
            }).collect::<Vec<_>>()}
        </div>
    }
}
