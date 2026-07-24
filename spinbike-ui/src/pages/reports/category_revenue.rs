use leptos::prelude::*;

use spinbike_core::reports::CategoryRevenue as CategoryRevenueRow;

use crate::i18n::{self, Lang};

/// Revenue-per-category breakdown (#255) — one row per active service (even
/// with zero sales this period), sorted by the server total_eur DESC. Shown
/// below the KPI cards on the Reports page. Uses the shared `.group`/
/// `.list-row` list primitive (same visual container styling as the KPI
/// cards) rather than a new bespoke style.
#[component]
pub fn CategoryRevenue(rows: ReadSignal<Vec<CategoryRevenueRow>>) -> impl IntoView {
    let lang = use_context::<ReadSignal<Lang>>().expect("Lang context");

    view! {
        <section class="group" data-testid="category-revenue">
            <div class="list-row">
                <div class="list-row__main">
                    <div class="list-row__title">
                        {move || i18n::t(lang.get(), "category_revenue_heading")}
                    </div>
                </div>
            </div>
            {move || {
                let r = rows.get();
                let l = lang.get();
                let total: f64 = r.iter().map(|c| c.total_eur).sum();
                let item_rows: Vec<_> = r
                    .iter()
                    .map(|c| {
                        let name = if l == Lang::Sk {
                            c.name_sk.clone()
                        } else {
                            c.name_en.clone()
                        };
                        let amount = format!("{:.2} \u{20ac}", c.total_eur);
                        view! {
                            <div class="list-row" data-testid="category-revenue-row">
                                <div class="list-row__main">
                                    <div class="list-row__title">{name}</div>
                                </div>
                                <div class="list-row__amount">{amount}</div>
                            </div>
                        }
                    })
                    .collect::<Vec<_>>();
                view! {
                    <>
                        {item_rows}
                        <div class="list-row" data-testid="category-revenue-total">
                            <div class="list-row__main">
                                <div class="list-row__title">
                                    {i18n::t(l, "category_revenue_total")}
                                </div>
                            </div>
                            <div class="list-row__amount">{format!("{total:.2} \u{20ac}")}</div>
                        </div>
                    </>
                }
            }}
        </section>
    }
}
