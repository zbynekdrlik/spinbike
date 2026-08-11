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
                service_name_sk: 'Mesačná permanentka',
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

    // #315: hold the log-visit POST in flight for a fixed window before
    // letting it reach the real server. On fast/unloaded CI hardware the
    // whole click -> loading=true -> POST -> loading=false cycle can
    // complete in single-digit ms (server + SQLite colocated with the test
    // client on the SAME runner) — fast enough that Playwright's poll can
    // miss the transient "disabled" state entirely (observed live: 5
    // polls over 1000ms, all read "enabled" — CI run 31363382651). The
    // client-side loading guard itself is correct (set synchronously
    // before the POST is even dispatched); this only controls the
    // network timing so the disabled window is reliably observable.
    // Same established pattern as edit-info-fixes.spec.ts's "Delay the
    // PUT so the save stays in flight long enough...".
    await page.route('**/api/payments/log-visit', async (route) => {
        await new Promise((r) => setTimeout(r, 400));
        return route.continue();
    });

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
                service_name_sk: 'Mesačná permanentka',
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

    // #315: hold the log-visit POST in flight so the SECOND click below
    // genuinely lands WHILE the first request is still outstanding — the
    // guard's actual intended condition. Without this, on fast/unloaded CI
    // hardware the first request can complete and re-enable the button
    // BEFORE the second real click even fires, making it a legitimate,
    // separate new click that correctly sends its own separate POST — not
    // a double-submit at all (observed live: "Received: 2", CI run
    // 31366920531). Same delay pattern as the test above.
    await page.route('**/api/payments/log-visit', async (route) => {
        await new Promise((r) => setTimeout(r, 500));
        return route.continue();
    });

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

async function seedActivePassBarcode(token: string, barcode: string): Promise<void> {
    const validUntilIso = new Date(Date.now() + 30 * 24 * 60 * 60 * 1000)
        .toISOString()
        .slice(0, 10);
    const seed = await fetch(`${BASE_URL}/api/test/seed-transactions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({
            barcode,
            entries: [{
                amount: -35.00,
                action: 'charge',
                service_name_sk: 'Mesačná permanentka',
                valid_until: validUntilIso,
            }],
        }),
    });
    if (!seed.ok) {
        throw new Error(`seedActivePassBarcode failed: ${seed.status} ${await seed.text()}`);
    }
}

// #344 finding 3: the auto-clear timer for each of do_topup/do_charge/
// do_log_visit compares the CURRENT banner text against the text it
// captured at schedule time (`msg.get_untracked() == m`). Two DIFFERENT
// customers logging the SAME service render IDENTICAL banner text ("Visit
// added: Fitness") — the FIRST action's stale 2.5s timer then matches the
// SECOND action's still-current banner and clears it early, ~1s ahead of
// when it should.
test('a stale timer from one visit must not early-clear a different, later visit with identical banner text', async ({
    page,
}) => {
    const msgs = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

    const RUN_TAG = `BANGEN${Math.random().toString(36).slice(2, 10).toUpperCase()}`;
    const barcodeA = `${RUN_TAG}A`;
    const barcodeB = `${RUN_TAG}B`;
    await seedActivePassBarcode(token, barcodeA);
    await seedActivePassBarcode(token, barcodeB);

    await page.goto('/staff');
    const search = page.locator('input[type="search"]').first();
    await search.waitFor();

    const banner = page.locator('.alert-success');

    // Customer A: log a Fitness visit.
    await search.fill(barcodeA);
    await expect(page.locator('[data-testid="search-result"]')).toHaveCount(1);
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
    const respA = page.waitForResponse(
        (r) => r.url().includes('/api/payments/log-visit') && r.request().method() === 'POST',
    );
    await page.locator('[data-testid="log-visit-btn"]').first().click();
    await respA;
    const tA = Date.now();
    await expect(banner).toHaveText('Visit added: Fitness');

    // ~1s later, switch to a DIFFERENT customer and log the SAME service —
    // identical banner text, but a distinct action with its own timer.
    await page.waitForTimeout(1000);
    await search.fill(barcodeB);
    await expect(page.locator('[data-testid="search-result"]')).toHaveCount(1);
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
    const respB = page.waitForResponse(
        (r) => r.url().includes('/api/payments/log-visit') && r.request().method() === 'POST',
    );
    await page.locator('[data-testid="log-visit-btn"]').first().click();
    await respB;
    const tB = Date.now();
    await expect(banner).toHaveText('Visit added: Fitness');

    // Wait until just past A's OWN 2.5s timer (measured from tA) — well
    // before B's 2.5s timer (measured from tB, which started ~1s later).
    // The banner must still be showing B's success.
    const untilPastA = tA + 2700 - Date.now();
    if (untilPastA > 0) await page.waitForTimeout(untilPastA);
    await expect(banner).toBeVisible();
    await expect(banner).toHaveText('Visit added: Fitness');

    // Wait until past B's own timer — now it clears.
    const untilPastB = tB + 2700 - Date.now();
    if (untilPastB > 0) await page.waitForTimeout(untilPastB);
    await expect(banner).not.toBeVisible({ timeout: 2000 });

    assertCleanConsole(msgs);
});
