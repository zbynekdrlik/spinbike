import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

// Issue #57: card panel header shows "Last visit: <date> (<relative>)" for
// cards with class visits, no element for cards with no class visits, and
// Quick Search results are ordered last-visit-DESC.
//
// Search-result rows only show the LAST 4 chars of the barcode + the
// card's name; our seeded cards have no name and unique base36 barcodes,
// so we can't easily identify them by search-result text alone. Instead,
// we click each nth(N) and verify the OPENED card panel's full barcode
// matches the expected card. If the sort is broken, the wrong card opens
// and the barcode mismatch fails the test.
test('last-visit display + Quick Search sort by last visit', async ({ page }) => {
    const msgs = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

    // Letter-heavy prefix avoids substring collision with the prod-synced dev DB
    // (real customer data — names, phones, barcodes — is in the search_text column).
    const RUN_TAG = `LVUNIQ${Math.random().toString(36).slice(2, 12).toUpperCase()}`;

    // Format a Date as the SQLite created_at literal "YYYY-MM-DD HH:MM:SS".
    const fmtTs = (d: Date): string =>
        `${d.toISOString().slice(0, 10)} 12:00:00`;

    // Note: `Date.toISOString()` is always UTC, so a `getTime() - N * 86400000`
    // offset can land on the wrong local calendar day in the 00:00-02:00 local
    // window. We pick 3 days ago (not 1) and assert with a `'days ago'` substring
    // so the test is robust under that edge.
    const today = new Date();
    const threeDaysAgo = new Date(today.getTime() - 3 * 24 * 60 * 60 * 1000);
    const hundredDaysAgo = new Date(today.getTime() - 100 * 24 * 60 * 60 * 1000);

    const cards = [
        {
            // Spinning visit 3 days ago → last_visit_at = 3d ago → "X days ago".
            // Barcode starts with "Zulu" — alphabetically LAST among the three.
            // With correct last_visit_at DESC sort, Zulu comes first because it
            // is the most recent class visitor.
            barcode: `Zulu${RUN_TAG}`,
            entries: [{
                amount: -3.30,
                action: 'charge',
                service_name_sk: 'Spinning',
                created_at: fmtTs(threeDaysAgo),
            }],
        },
        {
            // Spinning visit 100 days ago → last_visit_at = 100d ago →
            // bucket = months, N = floor(100/30) = 3 → "3 months ago".
            // Barcode starts with "Alpha" — alphabetically first, but sorts
            // SECOND because its visit is older than Zulu's.
            barcode: `Alpha${RUN_TAG}`,
            entries: [{
                amount: -3.30,
                action: 'charge',
                service_name_sk: 'Spinning',
                created_at: fmtTs(hundredDaysAgo),
            }],
        },
        {
            // Top-up only → NOT a class visit → last_visit_at = NULL → no
            // card-last-visit testid in DOM. NULL sinks to bottom of search.
            barcode: `Mike${RUN_TAG}`,
            entries: [{
                amount: 10.00,
                action: 'topup',
                service_name_sk: 'Občerstvenie',
            }],
        },
    ];

    for (const card of cards) {
        const seed = await fetch(`${BASE_URL}/api/test/seed-transactions`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
            body: JSON.stringify(card),
        });
        if (!seed.ok) {
            throw new Error(`seed failed for ${card.barcode}: ${seed.status} ${await seed.text()}`);
        }
    }

    await page.goto('/staff');
    const search = page.locator('input[type="search"]').first();
    await search.waitFor();
    await search.fill(RUN_TAG);

    const results = page.locator('[data-testid="search-result"]');
    await expect(results).toHaveCount(3);

    // ── ZuluTest: opens to "Last visit … (X days ago)" ──
    // Sort order is verified by asserting the OPENED panel's full barcode:
    // if last_visit_at DESC is broken and alphabetic fallback fires, the
    // wrong card opens at nth(0) and the barcode mismatch fails the test.
    await results.nth(0).click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
    // Verify sort order via the OPENED panel's barcode (full barcode is visible
    // here, unlike search results which only show the last 4 chars).
    await expect(page.locator('[data-testid="action-panel"] .desk-identity__barcode'))
        .toHaveText(`Zulu${RUN_TAG}`);
    const zuluLastVisit = page.locator('[data-testid="card-last-visit"]');
    await expect(zuluLastVisit).toBeVisible();
    await expect(zuluLastVisit).toContainText('Last visit');
    await expect(zuluLastVisit).toContainText('days ago');

    // ── AlphaTest: opens to "Last visit … (3 months ago)" ──
    await page.locator('[data-testid="action-panel"] button[title="close"]').click();
    await search.fill(RUN_TAG);
    await expect(results).toHaveCount(3);
    await results.nth(1).click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
    // Verify sort order via the OPENED panel's barcode (full barcode is visible
    // here, unlike search results which only show the last 4 chars).
    await expect(page.locator('[data-testid="action-panel"] .desk-identity__barcode'))
        .toHaveText(`Alpha${RUN_TAG}`);
    const alphaLastVisit = page.locator('[data-testid="card-last-visit"]');
    await expect(alphaLastVisit).toBeVisible();
    await expect(alphaLastVisit).toContainText('Last visit');
    await expect(alphaLastVisit).toContainText('3 months ago');

    // ── MikeTest: no card-last-visit testid in DOM ──
    await page.locator('[data-testid="action-panel"] button[title="close"]').click();
    await search.fill(RUN_TAG);
    await expect(results).toHaveCount(3);
    await results.nth(2).click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
    // Verify sort order via the OPENED panel's barcode (full barcode is visible
    // here, unlike search results which only show the last 4 chars).
    await expect(page.locator('[data-testid="action-panel"] .desk-identity__barcode'))
        .toHaveText(`Mike${RUN_TAG}`);
    await expect(page.locator('[data-testid="card-last-visit"]')).toHaveCount(0);

    assertCleanConsole(msgs);
});
