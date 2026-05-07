import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

// Issue #49: cards with credit < 0 must surface on the Desk in two ways:
// 1. Idle desk: a list under the search box (only when no card selected AND search empty).
// 2. Active search: dropdown rows for negative cards get the .search-result--negative class.
test('negative-balance list + search highlight', async ({ page }) => {
    const msgs = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

    // Letter-heavy prefix to dodge substring collision with prod-synced dev DB.
    const RUN_TAG = `NBLA${Math.random().toString(36).slice(2, 12).toUpperCase()}`;
    const fmtTs = (d: Date): string => `${d.toISOString().slice(0, 10)} 12:00:00`;
    const today = new Date();
    const yesterday = new Date(today.getTime() - 1 * 86400000);
    const lastWeek = new Date(today.getTime() - 7 * 86400000);
    const lastMonth = new Date(today.getTime() - 30 * 86400000);

    // Two negatives + one positive control.
    const cards = [
        { barcode: `Alpha${RUN_TAG}`, credit: -3.5,  visitAt: fmtTs(yesterday),  topupAt: fmtTs(lastWeek) },
        { barcode: `Bravo${RUN_TAG}`, credit: -10.0, visitAt: null,              topupAt: fmtTs(lastMonth) },
        { barcode: `Charlie${RUN_TAG}`, credit: 5.0, visitAt: null,              topupAt: fmtTs(yesterday) },
    ];

    for (const c of cards) {
        // 1. Set the credit (creates the card if missing).
        const credResp = await fetch(`${BASE_URL}/api/test/seed-credit`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
            body: JSON.stringify({ barcode: c.barcode, credit: c.credit }),
        });
        if (!credResp.ok) {
            throw new Error(`seed-credit failed for ${c.barcode}: ${credResp.status} ${await credResp.text()}`);
        }

        // 2. Optionally seed a visit and a topup so the row's last-visit/last-payment fields render.
        const entries: Array<{
            amount: number; action: string; service_name_sk: string; created_at?: string;
        }> = [];
        if (c.visitAt) {
            entries.push({ amount: 0, action: 'visit', service_name_sk: 'Spinning', created_at: c.visitAt });
        }
        if (c.topupAt) {
            entries.push({ amount: 5.0, action: 'topup', service_name_sk: 'Občerstvenie', created_at: c.topupAt });
        }
        if (entries.length > 0) {
            const txResp = await fetch(`${BASE_URL}/api/test/seed-transactions`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
                body: JSON.stringify({ barcode: c.barcode, entries }),
            });
            if (!txResp.ok) {
                throw new Error(`seed-transactions failed for ${c.barcode}: ${txResp.status} ${await txResp.text()}`);
            }
        }
    }

    await page.goto('/staff');

    // ---- Surface 1: idle desk list -----------------------------------------------
    const list = page.locator('[data-testid="negative-balance-list"]');
    await expect(list).toBeVisible({ timeout: 5000 });

    // ---- Surface 1a: heading carries count + sum (Issue #72) ---------------------
    // Heading format: "{HEADING}  ·  {count}  ·  {-DD.DD €}".
    // Dev DB is prod-synced — the count is whatever-is-already-there + our 2
    // negatives. We assert structural shape (regex), not exact values, with a
    // floor of >= 2 from our seeded rows.
    const heading = list.locator('.negative-balance-list__heading');
    await expect(heading).toBeVisible();
    const headingText = (await heading.textContent())?.trim() ?? '';
    const headingMatch = headingText.match(
        /^(Klienti v minuse|Customers with negative balance)\s+·\s+(\d+)\s+·\s+(-?\d+\.\d{2})\s+€$/,
    );
    expect(headingMatch, `heading text "${headingText}" did not match expected pattern`).not.toBeNull();
    const negCount = Number(headingMatch![2]);
    const negSum = Number(headingMatch![3]);
    expect(negCount).toBeGreaterThanOrEqual(2);
    expect(negSum).toBeLessThanOrEqual(-13.5); // Alpha (-3.50) + Bravo (-10.00) + any prior negatives

    const rows = list.locator('[data-testid="negative-balance-row"]');

    // The list shows ALL cards with credit<0 — prod-synced dev DB already has many.
    // Assert that BOTH our seeded negatives appear and the positive does NOT,
    // regardless of how many other rows are present.
    await expect(list.getByText(`Alpha${RUN_TAG}`, { exact: false })).toBeVisible();
    await expect(list.getByText(`Bravo${RUN_TAG}`, { exact: false })).toBeVisible();
    await expect(list.getByText(`Charlie${RUN_TAG}`, { exact: false })).toHaveCount(0);

    // Issue #78: single-line row layout — name + " (Posledna navsteva: ...)" + credit.
    // Drops last-payment from the row entirely.
    const alphaRowFull = rows.filter({ hasText: `Alpha${RUN_TAG}` }).first();
    const alphaText = (await alphaRowFull.textContent()) ?? '';
    expect(alphaText).toContain('(Posledna navsteva: ');
    expect(alphaText).not.toContain('Posledna platba');

    // Bravo (-10.00) must appear BEFORE Alpha (-3.50) in DOM order (most-negative-first sort).
    const bravoIdx = await rows.evaluateAll(
        (els, tag: string) => els.findIndex((e) => (e.textContent ?? '').includes(`Bravo${tag}`)),
        RUN_TAG,
    );
    const alphaIdx = await rows.evaluateAll(
        (els, tag: string) => els.findIndex((e) => (e.textContent ?? '').includes(`Alpha${tag}`)),
        RUN_TAG,
    );
    expect(bravoIdx).toBeGreaterThanOrEqual(0);
    expect(alphaIdx).toBeGreaterThan(bravoIdx);

    // ---- Surface 1b: clicking a row opens the action panel -----------------------
    const alphaRow = rows.filter({ hasText: `Alpha${RUN_TAG}` }).first();
    await alphaRow.click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible({ timeout: 2000 });
    // List hides because a card is now selected.
    await expect(list).toBeHidden();

    // Reset: clear selection by reloading. This brings us back to the idle desk state.
    await page.goto('/staff');
    await expect(list).toBeVisible();

    // ---- Surface 2: search highlight ---------------------------------------------
    const search = page.locator('input[type="search"]').first();
    await search.waitFor();
    await search.fill(RUN_TAG);
    await expect(list).toBeHidden(); // search active hides idle list

    const results = page.locator('[data-testid="search-result"]');
    await expect(results).toHaveCount(3, { timeout: 5000 });

    // The search-dropdown row only shows `…<last 4 barcode chars>` + name +
    // company; our seeded cards share the same last-4 (the RUN_TAG tail) and
    // have no name/company, so we can't filter by barcode/name. Each card has
    // a unique credit value rendered as e.g. "-3.50 €" — that string IS in
    // the row text and is unique within our 3-card scope.
    const charlieRow = results.filter({ hasText: '5.00 €' });
    await expect(charlieRow).toHaveCount(1);
    await expect(charlieRow).not.toHaveClass(/search-result--negative/);

    // Alpha (-3.50) & Bravo (-10.00) — must have the modifier class.
    const alphaRowSearch = results.filter({ hasText: '-3.50 €' });
    const bravoRowSearch = results.filter({ hasText: '-10.00 €' });
    await expect(alphaRowSearch).toHaveCount(1);
    await expect(bravoRowSearch).toHaveCount(1);
    await expect(alphaRowSearch).toHaveClass(/search-result--negative/);
    await expect(bravoRowSearch).toHaveClass(/search-result--negative/);

    // ---- Cleanup: clear search → list reappears ----------------------------------
    await search.fill('');
    await expect(list).toBeVisible();

    assertCleanConsole(msgs);
});
