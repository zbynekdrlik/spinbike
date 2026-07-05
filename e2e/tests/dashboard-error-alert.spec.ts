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
        await page.locator('[data-testid="block-button"]').click();
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
        await page.locator('[data-testid="block-button"]').click();
        await failedBlockResp;
        await expect(page.locator('.alert.alert-error')).toBeVisible({ timeout: 5000 });

        // 2. Stop failing the API, then successfully save an edit.
        await page.unroute('**/api/users/block');
        await page.locator('[data-testid="edit-info-button"]').click();
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
        await page.locator('[data-testid="block-button"]').click();
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

    /**
     * Deep code-review follow-up on #126 (PR #132): DeleteUserSheet's
     * on_saved callback (card_panel.rs) closes the panel via a SECOND path
     * — a bare `set_selected.set(None)` — that bypassed mod.rs's
     * clear_selection entirely, so it never got the msg/err clear that fix
     * added for the × button's close path.
     */
    test('deleting a user closes the panel and dismisses a stale error alert', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        const staffToken = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const user = await createUniqueUser(staffToken, 0, 'ED');

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
        await page.locator('[data-testid="block-button"]').click();
        await failedBlockResp;
        await expect(page.locator('.alert.alert-error')).toBeVisible({ timeout: 5000 });

        // Delete the (throwaway, freshly-created, zero-balance) test user —
        // the panel closes on success.
        await page.locator('[data-testid="delete-user-button"]').click();
        const sheet = page.locator('[data-testid="sheet-delete-user"]');
        await expect(sheet).toBeVisible();
        const deleteResp = page.waitForResponse(
            (r) => r.url().includes(`/api/users/${user.user_id}`) && r.request().method() === 'DELETE',
        );
        await page.locator('[data-testid="delete-user-confirm"]').click();
        await deleteResp;

        await expect(panel).not.toBeVisible();
        expect(await page.locator('.alert.alert-error').count()).toBe(0);

        const filtered = consoleMessages.filter((m) => !m.includes('500 ('));
        expect(filtered).toEqual([]);
    });

    /**
     * Regression test for #133: ActionForm's LOCAL validation error
     * (submitting an empty charge amount) renders the identical
     * `.alert.alert-error` markup as the dashboard's SHARED error channel
     * (mod.rs). Before #133, both boxes were indistinguishable to any
     * selector scoped only to the class — an E2E assertion of
     * `.alert.alert-error` count could not tell "shared channel" apart
     * from "local form validation". #133 added a `data-testid` to each
     * local error div (without touching the shared channel) so tooling can
     * target them independently. This test proves BOTH alerts can render
     * at once (the exact scenario the ticket describes) and that each one
     * is addressable by its own distinct `data-testid`.
     */
    test('a local ActionForm validation error is distinguishable from a stale shared-channel error (#133)', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);

        const staffToken = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const user = await createUniqueUser(staffToken, 0, 'EF');

        await page.goto('/staff');
        await page.waitForSelector('input[type="search"]');
        await page.fill('input[type="search"]', user.card_code);
        const result = page.locator('[data-testid="search-result"]').first();
        await expect(result).toBeVisible({ timeout: 3000 });
        await result.click();
        const panel = page.locator('[data-testid="action-panel"]');
        await expect(panel).toBeVisible();

        // 1. Trigger the SHARED dashboard error channel (mod.rs) via a
        // failed block action.
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
        await page.locator('[data-testid="block-button"]').click();
        await failedBlockResp;
        await expect(page.locator('.alert.alert-error')).toHaveCount(1);
        // The shared channel has no data-testid of its own — the local
        // action-form-error testid must NOT match it yet.
        expect(await page.locator('[data-testid="action-form-error"]').count()).toBe(0);

        // 2. WITHOUT dismissing the shared error, trigger ActionForm's OWN
        // local validation error by submitting an empty charge amount.
        await page.locator('[data-testid="charge-service"]').selectOption({ index: 1 });
        const amountInput = page.locator('[data-testid="charge-amount"]');
        await amountInput.focus();
        await amountInput.press('ControlOrMeta+a');
        await amountInput.press('Delete');
        await expect(amountInput).toHaveValue('');
        await page.locator('[data-testid="charge-submit"]').click();

        // Both alerts now render simultaneously — this is the exact
        // ambiguity #133 fixes: two `.alert.alert-error` boxes on screen,
        // but only ONE of them carries the `action-form-error` testid.
        await expect(page.locator('.alert.alert-error')).toHaveCount(2);
        const localError = page.locator('[data-testid="action-form-error"]');
        await expect(localError).toHaveCount(1);
        await expect(localError).toBeVisible();

        // The shared channel's error is still the OTHER box, still
        // carrying its own original text — proving the two are fully
        // independent (the local error didn't clear or replace it).
        const sharedError = page.locator('.alert.alert-error:not([data-testid="action-form-error"])');
        await expect(sharedError).toHaveCount(1);
        await expect(sharedError).toContainText('boom_test_error');

        const filtered = consoleMessages.filter((m) => !m.includes('500 ('));
        expect(filtered).toEqual([]);
    });
});
