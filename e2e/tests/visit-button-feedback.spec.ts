import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

// Issue #53: visit buttons (Fitness/Spinning shown when a card has an
// active monthly pass) had no per-press feedback — staff could not tell
// if the press registered or if the visit was logged. After the fix, the
// button greys out during the POST, the success banner shows
// "Visit added: Fitness", the button re-enables on response, and the
// banner auto-clears after 2.5s.

test('visit button shows loading + success banner + auto-clears', async ({ page }) => {
    const msgs = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

    // Letter-heavy unique tag to avoid collisions with prod-synced dev DB.
    const RUN_TAG = `VBFB${Math.random().toString(36).slice(2, 12).toUpperCase()}`;
    const barcode = `Visit${RUN_TAG}`;

    // Pass valid 30 days from now → days_remaining >= 0 → pass_is_active true.
    const today = new Date();
    const validUntil = new Date(today.getTime() + 30 * 24 * 60 * 60 * 1000);
    const validUntilIso = validUntil.toISOString().slice(0, 10);

    // Seed: monthly pass purchase (active), no other history.
    const seed = await fetch(`${BASE_URL}/api/test/seed-transactions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({
            barcode,
            entries: [{
                amount: -35.00,
                action: 'charge',
                service_name_sk: 'Mesačný preplatok',
                valid_until: validUntilIso,
            }],
        }),
    });
    if (!seed.ok) {
        throw new Error(`seed failed: ${seed.status} ${await seed.text()}`);
    }

    await page.goto('/staff');
    const search = page.locator('input[type="search"]').first();
    await search.waitFor();
    await search.fill(RUN_TAG);

    const results = page.locator('[data-testid="search-result"]');
    await expect(results).toHaveCount(1);
    await results.first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

    // Both visit buttons (Fitness, Spinning) should be visible because
    // pass_is_active is true. We click the first one (Fitness, by the
    // alphabetical sort applied in action_form.rs).
    const visitButtons = page.locator('[data-testid="log-visit-btn"]');
    await expect(visitButtons).toHaveCount(2);
    const fitnessBtn = visitButtons.first();
    await expect(fitnessBtn).toContainText('Fitness');

    // Click. Within 1s the disabled binding must have repainted.
    await fitnessBtn.click();
    await expect(fitnessBtn).toBeDisabled({ timeout: 1000 });

    // Within 2s the success banner appears with the visit-added text.
    const banner = page.locator('.alert-success');
    await expect(banner).toBeVisible({ timeout: 2000 });
    await expect(banner).toHaveText('Visit added: Fitness');

    // Within 3s after the click the POST resolves and the button re-enables.
    await expect(fitnessBtn).toBeEnabled({ timeout: 3000 });

    // After 3.5s the auto-clear has fired (2.5s + ~1s buffer).
    await expect(banner).not.toBeVisible({ timeout: 3500 });

    assertCleanConsole(msgs);
});

test('visit button re-entry guard: rapid double-click fires only one POST', async ({ page }) => {
    const msgs = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

    const RUN_TAG = `VBFG${Math.random().toString(36).slice(2, 12).toUpperCase()}`;
    const barcode = `Guard${RUN_TAG}`;

    const today = new Date();
    const validUntil = new Date(today.getTime() + 30 * 24 * 60 * 60 * 1000);
    const validUntilIso = validUntil.toISOString().slice(0, 10);

    const seed = await fetch(`${BASE_URL}/api/test/seed-transactions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({
            barcode,
            entries: [{
                amount: -35.00,
                action: 'charge',
                service_name_sk: 'Mesačný preplatok',
                valid_until: validUntilIso,
            }],
        }),
    });
    if (!seed.ok) {
        throw new Error(`seed failed: ${seed.status} ${await seed.text()}`);
    }

    // Track every POST to /api/payments/log-visit.
    const logVisitRequests: string[] = [];
    page.on('request', (req) => {
        if (req.url().endsWith('/api/payments/log-visit') && req.method() === 'POST') {
            logVisitRequests.push(req.url());
        }
    });

    await page.goto('/staff');
    const search = page.locator('input[type="search"]').first();
    await search.waitFor();
    await search.fill(RUN_TAG);
    await expect(page.locator('[data-testid="search-result"]')).toHaveCount(1);
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

    const fitnessBtn = page.locator('[data-testid="log-visit-btn"]').first();

    // Two clicks dispatched back-to-back. The first sets loading=true;
    // the second hits either the re-entry guard (loading still true at
    // get_untracked time) or the disabled DOM attribute. Either way,
    // exactly one POST should fire.
    await fitnessBtn.click();
    await fitnessBtn.click({ force: true });

    // Wait for the first POST to complete.
    await expect(page.locator('.alert-success')).toBeVisible({ timeout: 2000 });

    expect(logVisitRequests.length).toBe(1);

    assertCleanConsole(msgs);
});
