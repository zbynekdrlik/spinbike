import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

// Issue #57: card panel header shows "Last visit: <date> (today)" for cards with
// class visits, no element for cards with no class visits, and Quick Search results
// are ordered last-visit-DESC (NULL last_visit sinks to bottom).
//
// Seed strategy: because the test fixture endpoint always sets created_at = now,
// all three seeded cards get the same wall-clock last_visit_at (for class-visit
// cards) or NULL (for top-up-only card). Same-timestamp cards fall back to barcode
// ASC, so "Alpha…" < "Zulu…" < "Never…" (NULL).
test('last-visit display + Quick Search sort by last visit', async ({ page }) => {
    const msgs = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

    // Unique suffix so this run's three cards don't clash with other test runs.
    const RUN_TAG = `LV57${Date.now().toString().slice(-8)}`;

    const cards = [
        {
            // Has a Spinning visit → last_visit_at = now → shows "today".
            barcode: `Alpha${RUN_TAG}`,
            entries: [{ amount: -3.30, action: 'charge', service_name_sk: 'Spinning' }],
        },
        {
            // Has a Spinning visit → last_visit_at = now → shows "today".
            // Same timestamp as Alpha so barcode order wins: "Zulu" > "Alpha".
            barcode: `Zulu${RUN_TAG}`,
            entries: [{ amount: -3.30, action: 'charge', service_name_sk: 'Spinning' }],
        },
        {
            // Top-up only (Refreshments) → NOT a class visit → last_visit_at = NULL.
            // NULL sinks to the bottom of the sort regardless of barcode.
            barcode: `Never${RUN_TAG}`,
            entries: [{ amount: 10.00, action: 'topup', service_name_sk: 'Občerstvenie' }],
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

    // Navigate to staff dashboard and type RUN_TAG into Quick Search.
    // Only our three freshly-seeded cards match.
    await page.goto('/staff');
    const search = page.locator('input[type="search"]').first();
    await search.waitFor();
    await search.focus();
    await page.keyboard.type(RUN_TAG, { delay: 30 });

    const results = page.locator('[data-testid="search-result"]');
    await expect(results).toHaveCount(3);

    // Sort assertion:
    //   0 → Alpha (last_visit_at = now, barcode sorts first among ties)
    //   1 → Zulu  (last_visit_at = now, barcode sorts second)
    //   2 → Never (last_visit_at = NULL, always last)
    await expect(results.nth(0)).toContainText(`Alpha${RUN_TAG}`);
    await expect(results.nth(1)).toContainText(`Zulu${RUN_TAG}`);
    await expect(results.nth(2)).toContainText(`Never${RUN_TAG}`);

    // ── AlphaTest: open card panel, assert "Last visit" line shows "today" ──
    await results.nth(0).click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

    const alphaLastVisit = page.locator('[data-testid="card-last-visit"]');
    await expect(alphaLastVisit).toBeVisible();
    await expect(alphaLastVisit).toContainText('Last visit');
    await expect(alphaLastVisit).toContainText('today');

    // ── Close, re-open search, open ZuluTest: same assertions ──
    await page.locator('[data-testid="action-panel"] button[title="close"]').click();
    await search.focus();
    await search.fill('');
    await page.keyboard.type(RUN_TAG, { delay: 30 });
    await expect(results).toHaveCount(3);

    await results.nth(1).click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

    const zuluLastVisit = page.locator('[data-testid="card-last-visit"]');
    await expect(zuluLastVisit).toBeVisible();
    await expect(zuluLastVisit).toContainText('Last visit');
    await expect(zuluLastVisit).toContainText('today');

    // ── Close, re-open search, open NeverTest: NO card-last-visit element ──
    await page.locator('[data-testid="action-panel"] button[title="close"]').click();
    await search.focus();
    await search.fill('');
    await page.keyboard.type(RUN_TAG, { delay: 30 });
    await expect(results).toHaveCount(3);

    await results.nth(2).click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
    await expect(page.locator('[data-testid="card-last-visit"]')).toHaveCount(0);

    assertCleanConsole(msgs);
});
