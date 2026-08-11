import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

// #344 finding 5: two more Effect+spawn_local fetches with no RequestId
// staleness guard, found in a second scan of the same bug class as finding 1
// (edit_info_form.rs) — `spinbike-ui/src/util.rs`'s own RequestId doc
// comment says it was written for exactly this pattern (#66), but these two
// sites were never migrated.

test('reports/mod.rs: switching Day -> Week before the Day fetch resolves must not let it overwrite the Week data', async ({
    page,
}) => {
    const msgs = setupConsoleCheck(page);
    await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

    // The initial mount fires the DAY fetch (mode defaults to RangeMode::Day).
    // Delay it and poison its payload so we can detect if it ever wins.
    await page.route('**/api/reports/day**', async (route) => {
        await new Promise((r) => setTimeout(r, 1200));
        await route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({
                kpi: { spinning_visits: 0, attendance: 999999, passes_sold: 0, cash_in_eur: 0 },
                category_revenue: [],
                events: [],
                has_more: false,
            }),
        });
    });
    // Clicking "Week" fires a RANGE fetch (different endpoint) -- fast,
    // with the CORRECT/fresh data.
    await page.route('**/api/reports/range**', async (route) => {
        await route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({
                kpi: { spinning_visits: 0, attendance: 42, passes_sold: 0, cash_in_eur: 0 },
                category_revenue: [],
                events: [],
                has_more: false,
            }),
        });
    });

    await page.goto('/reports');
    await expect(page.locator('[data-testid="reports-page"]')).toBeVisible();
    // Switch to Week before the (delayed) Day response has landed.
    await page.locator('[data-testid="range-week"]').click();

    // Wait long enough for BOTH responses to have resolved.
    await page.waitForTimeout(1500);

    await expect(page.locator('[data-testid="kpi-attendance"] .kpi-card__value')).toHaveText('42');

    assertCleanConsole(msgs);
});

test('users_by_movement.rs: reopening the Users tab before an earlier fetch resolves must not let it win', async ({
    page,
}) => {
    const msgs = setupConsoleCheck(page);
    await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

    const PAGE = 50;
    const seedRow = (id: number) => ({
        id,
        name: `Seed ${id}`,
        card_code: null,
        last_movement_at: null,
        allow_self_entry: false,
    });

    let calls = 0;
    await page.route('**/api/users/by-last-movement**', async (route) => {
        calls += 1;
        if (calls === 1) {
            // Mount fetch (offset=0): exactly PAGE rows so has_more becomes
            // true and the "Show more" button appears.
            await route.fulfill({
                status: 200,
                contentType: 'application/json',
                body: JSON.stringify(Array.from({ length: PAGE }, (_, i) => seedRow(i + 1))),
            });
        } else if (calls === 2) {
            // First "Show more" (offset=PAGE) -- delayed and poisoned.
            await new Promise((r) => setTimeout(r, 1200));
            await route.fulfill({
                status: 200,
                contentType: 'application/json',
                body: JSON.stringify([{ id: -1, name: 'STALE PAGE', card_code: null, last_movement_at: null, allow_self_entry: false }]),
            });
        } else {
            // Second "Show more", fired before the first resolves (same
            // offset, since `offset` hasn't been updated yet) -- fast and
            // correct.
            await route.fulfill({
                status: 200,
                contentType: 'application/json',
                body: JSON.stringify([{ id: -2, name: 'FRESH PAGE', card_code: null, last_movement_at: null, allow_self_entry: false }]),
            });
        }
    });

    await page.goto('/reports');
    await expect(page.locator('[data-testid="reports-page"]')).toBeVisible();
    await page.locator('[data-testid="reports-tab-users"]').click();
    await expect(page.locator('[data-testid="user-row"]')).toHaveCount(PAGE);

    const showMore = page.locator('[data-testid="users-by-movement-show-more"]');
    await expect(showMore).toBeVisible();
    // Two rapid clicks -- the second forced through, mirroring this repo's
    // documented #60 sub-frame race window (the `disabled` binding may not
    // have repainted yet before the second click lands).
    await showMore.click();
    await showMore.click({ force: true });

    // Wait long enough for BOTH "Show more" responses to have resolved.
    await page.waitForTimeout(1500);

    // Exactly ONE extra row must have been applied (the LATEST dispatch),
    // never both (duplicated) and never the poisoned one.
    await expect(page.locator('[data-testid="user-row"]')).toHaveCount(PAGE + 1);
    await expect(page.locator('[data-testid="user-row"]', { hasText: 'FRESH PAGE' })).toHaveCount(1);
    await expect(page.locator('[data-testid="user-row"]', { hasText: 'STALE PAGE' })).toHaveCount(0);

    assertCleanConsole(msgs);
});
