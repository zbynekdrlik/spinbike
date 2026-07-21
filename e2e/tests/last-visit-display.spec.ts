import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

// Issue #57: card panel header shows "Last visit: <date> (<relative>)" for
// cards with class visits, no element for cards with no class visits, and
// Quick Search results are ordered last-visit-DESC.
//
// Issue #235: the Quick Search dropdown ALSO shows the last visit per row
// (`search-result-last-visit`), and BOTH the dropdown row and the card
// panel header highlight (`.visited-today`) when the last visit was TODAY —
// the signal that helps staff avoid logging a duplicate visit (#234).
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
            // Fitness visit TODAY (created_at omitted → datetime('now')) →
            // last_visit_at = today → most recent of the four → sorts FIRST.
            // Both the search dropdown row and (once opened) the card panel
            // header must carry the `.visited-today` highlight (#235).
            barcode: `Today${RUN_TAG}`,
            entries: [{
                amount: -3.30,
                action: 'charge',
                service_name_sk: 'Fitness',
            }],
        },
        {
            // Spinning visit 3 days ago → last_visit_at = 3d ago → "X days ago".
            // Barcode starts with "Zulu" — alphabetically LAST among the three.
            // With correct last_visit_at DESC sort, Zulu comes right after
            // Today because it is the next-most-recent class visitor.
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
            // THIRD because its visit is older than Zulu's.
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
    await expect(results).toHaveCount(4);

    // ── #235: dropdown rows, in last-visit-DESC order — Today, Zulu(3d),
    // Alpha(100d), Mike(never). Every row shows search-result-last-visit;
    // only Today's carries the .visited-today highlight class.
    const rowLastVisit = (idx: number) =>
        results.nth(idx).locator('[data-testid="search-result-last-visit"]');

    await expect(rowLastVisit(0)).toBeVisible();
    await expect(rowLastVisit(0)).toContainText('today');
    await expect(rowLastVisit(0)).toHaveClass(/visited-today/);

    await expect(rowLastVisit(1)).toContainText('days ago');
    await expect(rowLastVisit(1)).not.toHaveClass(/visited-today/);

    await expect(rowLastVisit(2)).toContainText('months ago');
    await expect(rowLastVisit(2)).not.toHaveClass(/visited-today/);

    await expect(rowLastVisit(3)).toContainText('never');
    await expect(rowLastVisit(3)).not.toHaveClass(/visited-today/);

    // ── TodayTest: opens to a HIGHLIGHTED "Last visit … (today)" ──
    await results.nth(0).click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
    await expect(page.locator('[data-testid="action-panel"] .card-title__barcode'))
        .toHaveText(`Today${RUN_TAG}`);
    const todayLastVisit = page.locator('[data-testid="card-last-visit"]');
    await expect(todayLastVisit).toBeVisible();
    await expect(todayLastVisit).toContainText('Last visit');
    await expect(todayLastVisit).toContainText('today');
    await expect(todayLastVisit).toHaveClass(/visited-today/);

    // ── ZuluTest: opens to "Last visit … (X days ago)", NOT highlighted ──
    // Sort order is verified by asserting the OPENED panel's full barcode:
    // if last_visit_at DESC is broken and alphabetic fallback fires, the
    // wrong card opens at nth(1) and the barcode mismatch fails the test.
    await page.locator('[data-testid="action-panel"] button[title="close"]').click();
    await search.fill(RUN_TAG);
    await expect(results).toHaveCount(4);
    await results.nth(1).click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
    await expect(page.locator('[data-testid="action-panel"] .card-title__barcode'))
        .toHaveText(`Zulu${RUN_TAG}`);
    const zuluLastVisit = page.locator('[data-testid="card-last-visit"]');
    await expect(zuluLastVisit).toBeVisible();
    await expect(zuluLastVisit).toContainText('Last visit');
    await expect(zuluLastVisit).toContainText('days ago');
    await expect(zuluLastVisit).not.toHaveClass(/visited-today/);

    // ── AlphaTest: opens to "Last visit … (3 months ago)", NOT highlighted ──
    await page.locator('[data-testid="action-panel"] button[title="close"]').click();
    await search.fill(RUN_TAG);
    await expect(results).toHaveCount(4);
    await results.nth(2).click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
    await expect(page.locator('[data-testid="action-panel"] .card-title__barcode'))
        .toHaveText(`Alpha${RUN_TAG}`);
    const alphaLastVisit = page.locator('[data-testid="card-last-visit"]');
    await expect(alphaLastVisit).toBeVisible();
    await expect(alphaLastVisit).toContainText('Last visit');
    await expect(alphaLastVisit).toContainText('3 months ago');
    await expect(alphaLastVisit).not.toHaveClass(/visited-today/);

    // ── MikeTest: no card-last-visit testid in DOM ──
    await page.locator('[data-testid="action-panel"] button[title="close"]').click();
    await search.fill(RUN_TAG);
    await expect(results).toHaveCount(4);
    await results.nth(3).click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
    await expect(page.locator('[data-testid="action-panel"] .card-title__barcode'))
        .toHaveText(`Mike${RUN_TAG}`);
    await expect(page.locator('[data-testid="card-last-visit"]')).toHaveCount(0);

    assertCleanConsole(msgs);
});
