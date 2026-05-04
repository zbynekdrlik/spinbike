import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

// Issue #57: card panel header shows "Last visit: <date> (<relative>)" and
// Quick Search results are ordered last-visit-DESC (NULL last_visit sinks to
// bottom). Three test cards with DIFFERENT timestamps verify the sort actually
// fires — the card barcodes are deliberately ordered so that alphabetic fallback
// would produce a DIFFERENT order than temporal sort:
//
//   Temporal (correct): Zulu (3d ago) → Alpha (100d ago) → Mike (NULL)
//   Alphabetic fallback: Alpha → Mike → Zulu (broken sort would give this)
//
// The assertion Zulu → Alpha → Mike therefore requires last_visit_at DESC to work.
test('last-visit display + Quick Search sort by last visit', async ({ page }) => {
    const msgs = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

    const RUN_TAG = `LV57${Date.now().toString().slice(-8)}`;

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
    await search.focus();
    await page.keyboard.type(RUN_TAG, { delay: 30 });

    const results = page.locator('[data-testid="search-result"]');
    await expect(results).toHaveCount(3);

    // Sort assertion (temporal, not alphabetic):
    //   0 → Zulu  (last_visit_at = 3 days ago, most recent)
    //   1 → Alpha (last_visit_at = 100 days ago)
    //   2 → Mike  (last_visit_at = NULL, always last)
    //
    // If last_visit_at DESC is broken and alphabetic fallback kicks in, the
    // order would be Alpha → Mike → Zulu — this assertion would fail.
    await expect(results.nth(0)).toContainText(`Zulu${RUN_TAG}`);
    await expect(results.nth(1)).toContainText(`Alpha${RUN_TAG}`);
    await expect(results.nth(2)).toContainText(`Mike${RUN_TAG}`);

    // ── ZuluTest: opens to "Last visit … (X days ago)" ──
    await results.nth(0).click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
    const zuluLastVisit = page.locator('[data-testid="card-last-visit"]');
    await expect(zuluLastVisit).toBeVisible();
    await expect(zuluLastVisit).toContainText('Last visit');
    await expect(zuluLastVisit).toContainText('days ago');

    // ── AlphaTest: opens to "Last visit … (3 months ago)" ──
    await page.locator('[data-testid="action-panel"] button[title="close"]').click();
    await search.focus();
    await search.fill('');
    await page.keyboard.type(RUN_TAG, { delay: 30 });
    await expect(results).toHaveCount(3);
    await results.nth(1).click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
    const alphaLastVisit = page.locator('[data-testid="card-last-visit"]');
    await expect(alphaLastVisit).toBeVisible();
    await expect(alphaLastVisit).toContainText('Last visit');
    await expect(alphaLastVisit).toContainText('3 months ago');

    // ── MikeTest: no card-last-visit testid in DOM ──
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
