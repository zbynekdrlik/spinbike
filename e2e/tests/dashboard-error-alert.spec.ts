import { test, expect } from '@playwright/test';
import { setupConsoleCheck, loginViaAPI, createUniqueUser } from './helpers';

const BASE_URL = 'http://localhost:8099';

/**
 * Regression test for #126: the dashboard's ephemeral status line rendered
 * ALL messages — success AND error — in the green `.alert-success` box.
 * A failed staff action (e.g. a block/save/void that errors) looked exactly
 * like a confirmation, which a staff member could reasonably misread as
 * "it worked". The fix routes error text through the existing red
 * `.alert-error` channel (mod.rs already renders it, just wasn't wired to
 * these call sites) instead of overloading the success channel.
 */
test.describe('Dashboard error alert styling (#126)', () => {
    test('a failed block action renders in the red error alert, not the green success alert', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);

        const staffToken = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const user = await createUniqueUser(staffToken, 0, 'EA');

        await page.goto('/staff');
        await page.waitForSelector('input[type="search"]');
        await page.fill('input[type="search"]', user.card_code);
        const result = page.locator('[data-testid="search-result"]').first();
        await expect(result).toBeVisible({ timeout: 3000 });
        await result.click();

        const panel = page.locator('[data-testid="action-panel"]');
        await expect(panel).toBeVisible();

        // Force the block API to fail so we can observe how the error renders.
        await page.route('**/api/users/block', (route) =>
            route.fulfill({
                status: 500,
                contentType: 'application/json',
                body: JSON.stringify({ error: 'boom_test_error' }),
            }),
        );

        const blockResp = page.waitForResponse(
            (r) => r.url().includes('/api/users/block') && r.request().method() === 'POST',
        );
        await page.getByRole('button', { name: 'Block', exact: true }).click();
        await blockResp;

        // The error must appear in the RED alert — exactly once — carrying
        // the server's error text.
        const errorAlert = page.locator('.alert.alert-error');
        await expect(errorAlert).toBeVisible({ timeout: 5000 });
        await expect(errorAlert).toHaveCount(1);
        await expect(errorAlert).toContainText('boom_test_error');

        // And the GREEN success alert must not be showing at all — this is
        // the exact bug: the error text landing in `.alert-success` instead.
        expect(await page.locator('.alert.alert-success').count()).toBe(0);

        // A forced 500 always logs a browser-level "Failed to load resource"
        // console error independent of how the app handles it — not a real
        // bug, filter it (same pattern as the intercepted 503 in
        // door-open.spec.ts).
        const filtered = consoleMessages.filter((m) => !m.includes('500 ('));
        expect(filtered).toEqual([]);
    });

    /**
     * Code-review follow-up on #126: splitting one shared status signal
     * into two independent ones (msg/err) can leave a STALE alert behind
     * once a later, unrelated action sets the OTHER one — e.g. a failed
     * block leaves the red alert up, then a successful edit-save shows the
     * green alert on top of it, and both render at once. Every action that
     * writes msg/err now clears the other channel first.
     */
    test('a later successful action clears a stale error alert (no stacking)', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        const staffToken = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const user = await createUniqueUser(staffToken, 0, 'EB');

        await page.goto('/staff');
        await page.waitForSelector('input[type="search"]');
        await page.fill('input[type="search"]', user.card_code);
        const result = page.locator('[data-testid="search-result"]').first();
        await expect(result).toBeVisible({ timeout: 3000 });
        await result.click();
        await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

        // 1. Make Block fail — red alert shows.
        await page.route('**/api/users/block', (route) =>
            route.fulfill({
                status: 500,
                contentType: 'application/json',
                body: JSON.stringify({ error: 'boom_test_error' }),
            }),
        );
        const failedBlockResp = page.waitForResponse(
            (r) => r.url().includes('/api/users/block') && r.request().method() === 'POST',
        );
        await page.getByRole('button', { name: 'Block', exact: true }).click();
        await failedBlockResp;
        await expect(page.locator('.alert.alert-error')).toBeVisible({ timeout: 5000 });

        // 2. Stop failing the API, then successfully save an edit.
        await page.unroute('**/api/users/block');
        await page.locator('button', { hasText: 'Edit info' }).click();
        const sheet = page.locator('[data-testid="sheet-edit-info"]');
        await expect(sheet).toBeVisible();
        const saveResp = page.waitForResponse(
            (r) => r.url().includes(`/api/users/${user.user_id}`) && r.request().method() === 'PUT',
        );
        await sheet.locator('button[type="submit"]').click();
        await saveResp;

        // The stale red alert must be gone; only the fresh green one shows.
        await expect(page.locator('.alert.alert-success')).toBeVisible({ timeout: 5000 });
        expect(await page.locator('.alert.alert-error').count()).toBe(0);

        const filtered = consoleMessages.filter((m) => !m.includes('500 ('));
        expect(filtered).toEqual([]);
    });

    test('closing the action panel dismisses a stale error alert', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        const staffToken = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const user = await createUniqueUser(staffToken, 0, 'EC');

        await page.goto('/staff');
        await page.waitForSelector('input[type="search"]');
        await page.fill('input[type="search"]', user.card_code);
        const result = page.locator('[data-testid="search-result"]').first();
        await expect(result).toBeVisible({ timeout: 3000 });
        await result.click();
        const panel = page.locator('[data-testid="action-panel"]');
        await expect(panel).toBeVisible();

        await page.route('**/api/users/block', (route) =>
            route.fulfill({
                status: 500,
                contentType: 'application/json',
                body: JSON.stringify({ error: 'boom_test_error' }),
            }),
        );
        const failedBlockResp = page.waitForResponse(
            (r) => r.url().includes('/api/users/block') && r.request().method() === 'POST',
        );
        await page.getByRole('button', { name: 'Block', exact: true }).click();
        await failedBlockResp;
        await expect(page.locator('.alert.alert-error')).toBeVisible({ timeout: 5000 });

        // Close the panel via its × button — the stale red alert must not
        // survive on the now-idle dashboard screen.
        await panel.locator('button[title="close"]').click();
        await expect(panel).not.toBeVisible();
        expect(await page.locator('.alert.alert-error').count()).toBe(0);

        const filtered = consoleMessages.filter((m) => !m.includes('500 ('));
        expect(filtered).toEqual([]);
    });
});
