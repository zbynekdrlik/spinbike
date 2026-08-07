import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Card dashboard (staff /staff)', () => {
    test('search by barcode tail selects the matching card', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff');
        await page.waitForSelector('input[type="search"]');

        await page.fill('input[type="search"]', '1001');
        // The debounced search fires at ~250ms — wait for a result row.
        const result = page.locator('[data-testid="search-result"]').first();
        await expect(result).toBeVisible({ timeout: 3000 });
        await expect(result).toContainText('Jana Testova');
        await expect(result).toContainText('1001');

        await result.click();
        const panel = page.locator('[data-testid="action-panel"]');
        await expect(panel).toBeVisible();
        await expect(panel).toContainText('Jana Testova');
        await expect(panel).toContainText('70701001');

        assertCleanConsole(consoleMessages);
    });

    // Regression test for the #39 collision class recurring (CI run
    // 31178634087 — see the PR #290 root-cause comment for full detail).
    // dashboard.spec.ts's `search_text LIKE '%1001%'` search is
    // UNANCHORED: it matches ANY user's name+company+card_code anywhere in
    // the shared E2E DB, and when neither matching row's card_code
    // PREFIX-matches the query, ordering falls through to plain
    // `name ASC`. Manufacture a deterministic stand-in for the class of
    // polluter that broke this ('AAA Polluter', card containing '1001',
    // sorts before 'Jana Testova' alphabetically) directly in this test so
    // its outcome never depends on another spec's random Date.now() value
    // happening to land inside the search window.
    test('search by barcode tail is not fooled by an unrelated card whose id happens to contain the same digits', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);

        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const seed = await fetch(`${BASE_URL}/api/users`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
            body: JSON.stringify({
                name: 'AAA Polluter',
                initial_credit: 1,
                card_code: 'ZZ-99991001999',
            }),
        });
        if (!seed.ok) throw new Error(`seed polluter failed: ${seed.status} ${await seed.text()}`);

        await page.goto('/staff');
        await page.waitForSelector('input[type="search"]');
        await page.fill('input[type="search"]', '1001');

        // Both 'AAA Polluter' (card ZZ-99991001999) and 'Jana Testova' (card
        // 70701001) match the unanchored '%1001%' search. Neither card_code
        // is a PREFIX match for '1001', so the query's ORDER BY falls
        // through to plain name ASC — 'AAA' sorts before 'Jana', so a blind
        // `.first()` picks the WRONG card.
        const result = page.locator('[data-testid="search-result"]').first();
        await expect(result).toBeVisible({ timeout: 3000 });
        await expect(result).toContainText('Jana Testova');
        await expect(result).toContainText('70701001');

        await result.click();
        const panel = page.locator('[data-testid="action-panel"]');
        await expect(panel).toBeVisible();
        await expect(panel).toContainText('Jana Testova');

        assertCleanConsole(consoleMessages);
    });

    test('search by surname finds the card', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff');
        await page.waitForSelector('input[type="search"]');

        await page.fill('input[type="search"]', 'Novotna');
        const result = page.locator('[data-testid="search-result"]').first();
        await expect(result).toBeVisible({ timeout: 3000 });
        await expect(result).toContainText('Eva Novotna');

        assertCleanConsole(consoleMessages);
    });

    test('search by company returns multiple results', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff');
        await page.waitForSelector('input[type="search"]');

        await page.fill('input[type="search"]', 'TestCorp');
        await expect(page.locator('[data-testid="search-result"]')).toHaveCount(2, { timeout: 3000 });

        assertCleanConsole(consoleMessages);
    });

    test('no matches shows add-person CTA', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff');
        await page.waitForSelector('input[type="search"]');

        await page.fill('input[type="search"]', 'nonexistent-xyz-qqq');
        await expect(page.getByText('No matches')).toBeVisible({ timeout: 3000 });
        // The "Add Person" button should appear inline with the empty state.
        await expect(page.locator('[data-testid="add-person-submit"]').first()).toBeVisible();

        assertCleanConsole(consoleMessages);
    });

    test('quick top-up +20 updates displayed balance', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff');
        await page.waitForSelector('input[type="search"]');

        // Fetch baseline balance for 70702002.
        const before = await page.evaluate(async () => {
            const token = localStorage.getItem('spinbike_token');
            const r = await fetch('/api/users/lookup/70702002', {
                headers: { Authorization: `Bearer ${token}` },
            });
            return (await r.json()).credit as number;
        });

        await page.fill('input[type="search"]', '2002');
        const result = page.locator('[data-testid="search-result"]').first();
        await expect(result).toBeVisible({ timeout: 3000 });
        await result.click();

        await page.locator('[data-testid="charge-amount"]').fill('30');
        await page.locator('[data-testid="topup-submit"]').click();

        // Wait for panel to reflect the new credit.
        await expect(page.locator('[data-testid="action-panel"]')).toContainText(
            `${(before + 30).toFixed(2)} €`,
            { timeout: 5000 }
        );

        // Verify server-side persistence.
        const after = await page.evaluate(async () => {
            const token = localStorage.getItem('spinbike_token');
            const r = await fetch('/api/users/lookup/70702002', {
                headers: { Authorization: `Bearer ${token}` },
            });
            return (await r.json()).credit as number;
        });
        expect(after).toBeCloseTo(before + 30, 2);

        assertCleanConsole(consoleMessages);
    });

    test('charge for service reduces balance', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff');
        await page.waitForSelector('input[type="search"]');

        // Baseline.
        const before = await page.evaluate(async () => {
            const token = localStorage.getItem('spinbike_token');
            const r = await fetch('/api/users/lookup/70703003', {
                headers: { Authorization: `Bearer ${token}` },
            });
            return (await r.json()).credit as number;
        });

        // Use the FULL barcode for the search instead of "3003". The
        // substring "3003" can collide with timestamp-based barcodes
        // generated by other tests (e.g. card-action-form-language uses
        // `LNG-${Date.now()}`, which today happens to contain "3003").
        await page.fill('input[type="search"]', '70703003');
        const result = page.locator('[data-testid="search-result"]').first();
        await expect(result).toBeVisible({ timeout: 3000 });
        await result.click();

        // Pick the first (non-placeholder) service — global-setup seeds "Spinning".
        const select = page.locator('[data-testid="charge-service"]');
        await select.selectOption({ index: 1 });

        // Staff types the amount every time (#17 — no predefined prices).
        // 5 is below the card's starting balance so the charge always succeeds.
        const amountInput = page.locator('[data-testid="charge-amount"]');
        await amountInput.fill('5');

        // Wait for the charge POST to complete before asserting the toast,
        // otherwise the alert visibility can race the API round-trip under
        // parallel CI load (band-aid would be a longer timeout — this syncs
        // on the actual signal instead).
        const chargeResp = page.waitForResponse(
            (r) => r.url().includes('/api/payments/charge') && r.request().method() === 'POST',
        );
        await page.locator('[data-testid="charge-submit"]').click();
        const resp = await chargeResp;
        expect(resp.ok()).toBe(true);

        await expect(page.locator('.alert-success')).toBeVisible();

        const after = await page.evaluate(async () => {
            const token = localStorage.getItem('spinbike_token');
            const r = await fetch('/api/users/lookup/70703003', {
                headers: { Authorization: `Bearer ${token}` },
            });
            return (await r.json()).credit as number;
        });
        expect(after).toBeCloseTo(before - 5, 2);

        assertCleanConsole(consoleMessages);
    });
});
