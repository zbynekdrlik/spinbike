import { test, expect } from '@playwright/test';
import { setupConsoleCheck, loginViaAPI, createUniqueUser } from './helpers';

const BASE_URL = 'http://localhost:8099';

/**
 * Two regressions reported together from the field (Valovic edit-save):
 *
 *  1. A rejected edit-save (e.g. the 409 email-uniqueness conflict — the
 *     email is already held by another account) was set on the dashboard's
 *     SHARED red alert (mod.rs), which renders BEHIND the edit sheet's
 *     `z-index: 200` blur backdrop. The sheet stays open on a save error, so
 *     the alert was never visible — the operator saw the email simply "not
 *     save" with no reason. The fix renders the error INSIDE the sheet.
 *
 *  2. The edit sheet showed a "set new password" field for CUSTOMER targets.
 *     Customers are passwordless (magic-link only, per the onboarding
 *     design), so the field is meaningless for them — it only belongs on
 *     admin/staff targets who sign in via /api/auth/login. It was gated on
 *     the CALLER being admin, not on the TARGET's role.
 */
test.describe('Edit-info form field fixes', () => {
    test('a rejected save shows the error INSIDE the still-open sheet, not hidden behind the backdrop', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);

        const staffToken = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const user = await createUniqueUser(staffToken, 0, 'EI');

        await page.goto('/staff');
        await page.waitForSelector('input[type="search"]');
        await page.fill('input[type="search"]', user.card_code);
        const result = page.locator('[data-testid="search-result"]').first();
        await expect(result).toBeVisible({ timeout: 3000 });
        await result.click();
        await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

        await page.locator('[data-testid="edit-info-button"]').click();
        const sheet = page.locator('[data-testid="sheet-edit-info"]');
        await expect(sheet).toBeVisible();

        // Force the save to fail with the real server email-collision 409.
        await page.route(`**/api/users/${user.user_id}`, (route) => {
            if (route.request().method() === 'PUT') {
                return route.fulfill({
                    status: 409,
                    contentType: 'application/json',
                    body: JSON.stringify({ error: 'A user with this email already exists' }),
                });
            }
            return route.continue();
        });

        // Type a (would-be) colliding email and save.
        await sheet.locator('input[type="email"]').fill('luki.valovic@gmail.com');
        const saveResp = page.waitForResponse(
            (r) => r.url().includes(`/api/users/${user.user_id}`) && r.request().method() === 'PUT',
        );
        await sheet.locator('button[type="submit"]').click();
        await saveResp;

        // The error must be visible INSIDE the sheet (the whole bug), and the
        // sheet must stay open so the operator can fix the email inline.
        const inSheetError = sheet.locator('[data-testid="edit-info-error"]');
        await expect(inSheetError).toBeVisible({ timeout: 5000 });
        // ...and carry the plain-language collision message, not a silent /
        // raw failure (proves the 409 was mapped, not swallowed).
        await expect(inSheetError).toContainText('already used by another account');
        await expect(sheet).toBeVisible();

        // The 409 logs a browser-level "Failed to load resource" that the
        // shared helper already filters (4xx). Nothing else should appear.
        expect(consoleMessages).toEqual([]);
    });

    test('admin editing a CUSTOMER sees no password field (customers are passwordless)', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);

        const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        const user = await createUniqueUser(adminToken, 0, 'PW');

        await page.goto('/staff');
        await page.waitForSelector('input[type="search"]');
        await page.fill('input[type="search"]', user.card_code);
        const result = page.locator('[data-testid="search-result"]').first();
        await expect(result).toBeVisible({ timeout: 3000 });
        await result.click();
        await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

        await page.locator('[data-testid="edit-info-button"]').click();
        const sheet = page.locator('[data-testid="sheet-edit-info"]');
        await expect(sheet).toBeVisible();

        // The password field must NOT render for a customer target...
        await expect(sheet.locator('[data-testid="user-edit-password"]')).toHaveCount(0);
        // ...but the admin-only "allow self entry" control (the customer-only
        // counterpart in the same admin block) MUST still render — proving the
        // fix hid ONLY the password field, not the whole admin section.
        await expect(sheet.locator('[data-testid="user-edit-allow-self-entry"]')).toBeVisible();

        expect(consoleMessages).toEqual([]);
    });
});
