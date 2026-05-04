import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

// Per-card Overview tab: assert KPI numbers + chart bars render correctly
// after seeding a known transaction shape.
//
// Seeded scenario (this calendar month, in the test DB):
//   - 1 Spinning visit @ -3.30 € (counts as visit)
//   - 1 Fitness  visit @ -5.00 € (counts as visit)
//   - 1 Refreshments charge      (NOT a visit)
//   - 1 top-up   @ +50.00 €
//
// Expectation:
//   This month → Visits=2, Topped up=50.00 €
//   This year  → same (everything is this year)
//   All time   → same
//   Visits chart contains a row with value "2" for the current month
//   Top-ups chart contains a row with value "50.00 €" for the current month
test('per-card Overview tab shows correct KPIs and bars', async ({ page }) => {
    const msgs = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
    const barcode = `OV-${Date.now()}`;

    const seed = await fetch(`${BASE_URL}/api/test/seed-transactions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({
            barcode,
            entries: [
                { amount: -3.30, action: 'charge', service_name_sk: 'Spinning' },
                { amount: -5.00, action: 'charge', service_name_sk: 'Fitness' },
                { amount: -2.50, action: 'charge', service_name_sk: 'Občerstvenie' },
                { amount: 50.00, action: 'topup',  service_name_sk: 'Občerstvenie' },
            ],
        }),
    });
    if (!seed.ok) throw new Error(`seed failed: ${seed.status} ${await seed.text()}`);

    // Open the card via search → action panel.
    await page.goto('/staff');
    const search = page.locator('input[type="search"]');
    await search.waitFor();
    await search.focus();
    await page.keyboard.type(barcode, { delay: 30 });
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

    // Click the Overview tab. With English forced by loginViaAPI, the tab
    // label reads "Overview".
    await page.locator('[data-testid="tab-overview"]').click();
    await expect(page.locator('[data-testid="overview-tab"]')).toBeVisible();

    // KPI table — strict checks via data-testid.
    await expect(
        page.locator('[data-testid="overview-visits-overview_period_month"]')
    ).toHaveText('2');
    await expect(
        page.locator('[data-testid="overview-topup-overview_period_month"]')
    ).toHaveText('50.00 €');
    await expect(
        page.locator('[data-testid="overview-visits-overview_period_year"]')
    ).toHaveText('2');
    await expect(
        page.locator('[data-testid="overview-topup-overview_period_all"]')
    ).toHaveText('50.00 €');

    // Both charts render exactly 12 rows.
    await expect(page.locator('[data-testid="stats-visits-row"]')).toHaveCount(12);
    await expect(page.locator('[data-testid="stats-topup-row"]')).toHaveCount(12);

    // The current-month row in the visits chart shows "2"; the current-month
    // row in the top-ups chart shows "50.00 €". Charts render newest-first
    // so the first row IS the current month.
    await expect(
        page.locator('[data-testid="stats-visits-row"]').first()
    ).toContainText('2');
    await expect(
        page.locator('[data-testid="stats-topup-row"]').first()
    ).toContainText('50.00 €');

    assertCleanConsole(msgs);
});
