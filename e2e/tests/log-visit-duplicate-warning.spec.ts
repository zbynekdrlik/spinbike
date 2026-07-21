import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

// Issue #234: staff logs a visit for a client who already has a recorded
// visit/entry TODAY (from a prior manual log-visit, or from a door
// self-entry) — CEO was creating duplicate visit rows because nothing
// warned him. The fix is warn + explicit confirm, NOT a hard block (a
// genuine second visit in one day — e.g. morning Fitness + evening
// Spinning — is legitimate).

async function seedActivePass(token: string, barcode: string) {
    const validUntil = new Date(Date.now() + 30 * 24 * 60 * 60 * 1000)
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
                valid_until: validUntil,
            }],
        }),
    });
    if (!seed.ok) {
        throw new Error(`seedActivePass failed: ${seed.status} ${await seed.text()}`);
    }
}

test('duplicate same-day manual visit: warns, Cancel logs nothing, Add anyway logs the second', async ({ page }) => {
    const msgs = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

    const RUN_TAG = `DUPV${Math.random().toString(36).slice(2, 12).toUpperCase()}`;
    const barcode = `Visit${RUN_TAG}`;
    await seedActivePass(token, barcode);

    await page.goto('/staff');
    const search = page.locator('input[type="search"]').first();
    await search.waitFor();
    await search.fill(RUN_TAG);

    const results = page.locator('[data-testid="search-result"]');
    await expect(results).toHaveCount(1);
    await results.first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

    const visitButtons = page.locator('[data-testid="log-visit-btn"]');
    await expect(visitButtons).toHaveCount(2);
    const fitnessBtn = visitButtons.first();
    await expect(fitnessBtn).toContainText('Fitness');

    const confirm = page.locator('[data-testid="visit-confirm"]');
    const banner = page.locator('.alert-success');

    // First click: no prior visit today — logs cleanly, no confirm shown.
    await fitnessBtn.click();
    await expect(banner).toBeVisible({ timeout: 2000 });
    await expect(banner).toHaveText('Visit added: Fitness');
    await expect(confirm).not.toBeVisible();
    await expect(fitnessBtn).toBeEnabled({ timeout: 3000 });

    // Second click, same day: must warn instead of silently logging a
    // duplicate. Message must name the source (manual, not door).
    await fitnessBtn.click();
    await expect(confirm).toBeVisible({ timeout: 2000 });
    await expect(page.locator('[data-testid="visit-confirm-message"]')).toContainText('logged manually');

    // Cancel: dialog closes, nothing was logged.
    await page.locator('[data-testid="visit-confirm-cancel"]').click();
    await expect(confirm).not.toBeVisible();

    // Clicking again reproduces the SAME warning (still only one visit today).
    await fitnessBtn.click();
    await expect(confirm).toBeVisible({ timeout: 2000 });

    // Confirm "Add anyway": the second (legitimate) visit is logged.
    await page.locator('[data-testid="visit-confirm-force"]').click();
    await expect(confirm).not.toBeVisible();
    await expect(banner).toBeVisible({ timeout: 2000 });
    await expect(banner).toHaveText('Visit added: Fitness');

    assertCleanConsole(msgs);
});

test('duplicate same-day visit sourced from a door entry: warns with "via door"', async ({ page }) => {
    const msgs = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

    const RUN_TAG = `DUPVDOOR${Math.random().toString(36).slice(2, 10).toUpperCase()}`;
    const barcode = `Visit${RUN_TAG}`;
    await seedActivePass(token, barcode);

    // Seed a today's door-style visit row directly (note: 'door: 1st') on the
    // Fitness service — migration V16 re-tags Fitness as the door route's
    // kind='single_entry' row, so a real door press lands here too.
    const seedDoor = await fetch(`${BASE_URL}/api/test/seed-transactions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({
            barcode,
            entries: [{
                amount: 0.0,
                action: 'visit',
                service_name_sk: 'Fitness',
                note: 'door: 1st',
            }],
        }),
    });
    if (!seedDoor.ok) {
        throw new Error(`seed door row failed: ${seedDoor.status} ${await seedDoor.text()}`);
    }

    await page.goto('/staff');
    const search = page.locator('input[type="search"]').first();
    await search.waitFor();
    await search.fill(RUN_TAG);

    const results = page.locator('[data-testid="search-result"]');
    await expect(results).toHaveCount(1);
    await results.first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

    const visitButtons = page.locator('[data-testid="log-visit-btn"]');
    await expect(visitButtons).toHaveCount(2);
    const fitnessBtn = visitButtons.first();
    await expect(fitnessBtn).toContainText('Fitness');

    const confirm = page.locator('[data-testid="visit-confirm"]');
    await fitnessBtn.click();
    await expect(confirm).toBeVisible({ timeout: 2000 });
    await expect(page.locator('[data-testid="visit-confirm-message"]')).toContainText('via door');

    // Cancel to leave no extra side effects.
    await page.locator('[data-testid="visit-confirm-cancel"]').click();
    await expect(confirm).not.toBeVisible();

    assertCleanConsole(msgs);
});

// Review follow-up to #236 (#234/#235): the "Pridat aj tak" force-retry
// button lacked the same re-entry guard the primary visit button has —
// its `disabled=move || loading.get()` binding can lag a fast double-tap,
// letting two POSTs through and logging a duplicate visit row. Same
// technique as `visit-button-feedback.spec.ts`'s primary-button guard test:
// track every POST to log-visit and assert the force-retry click fires
// exactly one.
test('force-retry "Add anyway" button re-entry guard: rapid double-click fires only one POST', async ({ page }) => {
    const msgs = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

    const RUN_TAG = `DUPVFG${Math.random().toString(36).slice(2, 10).toUpperCase()}`;
    const barcode = `Visit${RUN_TAG}`;
    await seedActivePass(token, barcode);

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

    // First click: no prior visit today — logs cleanly (1st POST).
    await fitnessBtn.click();
    await expect(page.locator('.alert-success')).toBeVisible({ timeout: 2000 });

    // Second click, same day: raises the confirm dialog instead of logging
    // (2nd POST, gets the 409 conflict).
    await fitnessBtn.click();
    const confirm = page.locator('[data-testid="visit-confirm"]');
    await expect(confirm).toBeVisible({ timeout: 2000 });
    expect(logVisitRequests.length).toBe(2);

    const forceBtn = page.locator('[data-testid="visit-confirm-force"]');

    // Two clicks dispatched back-to-back on the force-retry button. The
    // guard must make the second a no-op — exactly one more (force) POST.
    await forceBtn.click();
    await forceBtn.click({ force: true });

    await expect(page.locator('.alert-success')).toBeVisible({ timeout: 2000 });
    await expect(confirm).not.toBeVisible();

    expect(logVisitRequests.length).toBe(3);

    assertCleanConsole(msgs);
});
