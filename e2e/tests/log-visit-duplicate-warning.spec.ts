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
