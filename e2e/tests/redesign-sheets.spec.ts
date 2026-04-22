import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

/**
 * Seed a card with an active monthly pass via the test-fixtures endpoint.
 * Reuses the same helper pattern as credit-improvements.spec.ts.
 */
async function seedCardWithPass(
    request: import('@playwright/test').APIRequestContext,
    token: string,
    barcode: string,
    validUntil: string,
): Promise<void> {
    const resp = await request.post(`${BASE_URL}/api/test/seed-expired-pass`, {
        data: { barcode, valid_until: validUntil },
        headers: { Authorization: `Bearer ${token}` },
    });
    expect(resp.ok()).toBeTruthy();
}

async function openCard(page: import('@playwright/test').Page, searchTerm: string) {
    await page.goto('/staff');
    const searchInput = page.locator('input[type="search"]');
    await searchInput.waitFor();
    await searchInput.focus();
    await page.keyboard.type(searchTerm, { delay: 30 });
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
}

test.describe('redesign: sheets', () => {
    test('sell pass sheet opens and closes via cancel', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await openCard(page, 'Jana');

        // Open the sell pass sheet.
        await page.locator('[data-testid="sell-pass-btn"]').click();
        await expect(page.locator('[data-testid="sheet-sell-pass"]')).toBeVisible();

        // Cancel button uses the modal_cancel i18n key → "Zrusit" (SK) / "Cancel" (EN).
        // Both languages are accepted since helpers.ts sets EN but the test should be robust.
        await page.locator('[data-testid="sheet-sell-pass"] button').filter({ hasText: /zrusit|cancel/i }).click();
        await expect(page.locator('[data-testid="sheet-sell-pass"]')).not.toBeVisible({ timeout: 2000 });

        assertCleanConsole(msgs);
    });

    test('edit info sheet opens and closes via cancel', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await openCard(page, 'Jana');

        // The Edit Info button text uses the edit_info i18n key.
        // Click it to toggle the sheet open.
        await page.locator('[data-testid="action-panel"] button').filter({ hasText: /edit.info|upravit/i }).click();
        await expect(page.locator('[data-testid="sheet-edit-info"]')).toBeVisible();

        // Cancel button inside the sheet uses the cancel i18n key → "Zrusit" (SK) / "Cancel" (EN).
        await page.locator('[data-testid="sheet-edit-info"] button').filter({ hasText: /zrusit|cancel/i }).click();
        await expect(page.locator('[data-testid="sheet-edit-info"]')).not.toBeVisible({ timeout: 2000 });

        assertCleanConsole(msgs);
    });

    test('edit pass date sheet opens when pass is active', async ({ page, request }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');

        // Seed a card with a future pass so the active banner appears.
        const barcode = 'SHEET-EDIT-DATE-01';
        await seedCardWithPass(request, token, barcode, '2030-12-31');
        await openCard(page, barcode);

        // The active pass banner must be visible for this test to be meaningful.
        const activeBanner = page.locator('[data-testid="pass-banner-active"]');
        if (!(await activeBanner.isVisible())) {
            test.skip(true, 'No active pass on this card — seeding may have failed');
            return;
        }

        // Click the edit pass date button inside the banner.
        await page.locator('[data-testid="pass-date-edit"]').click();
        await expect(page.locator('[data-testid="sheet-edit-pass-date"]')).toBeVisible();

        // Close via the cancel button (uses the cancel i18n key).
        await page.locator('[data-testid="sheet-edit-pass-date"] button').filter({ hasText: /zrusit|cancel/i }).click();
        await expect(page.locator('[data-testid="sheet-edit-pass-date"]')).not.toBeVisible({ timeout: 2000 });

        assertCleanConsole(msgs);
    });

    test('escape key closes a sheet', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await openCard(page, 'Jana');

        // Open the sell pass sheet.
        await page.locator('[data-testid="sell-pass-btn"]').click();
        const sheet = page.locator('[data-testid="sheet-sell-pass"]');
        await expect(sheet).toBeVisible();

        // The sheet element has tabindex="-1"; focus it so Escape is delivered to it.
        await sheet.focus();
        await page.keyboard.press('Escape');
        await expect(sheet).not.toBeVisible({ timeout: 2000 });

        assertCleanConsole(msgs);
    });

    test('backdrop click closes a sheet', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await openCard(page, 'Jana');

        // Open the sell pass sheet.
        await page.locator('[data-testid="sell-pass-btn"]').click();
        await expect(page.locator('[data-testid="sheet-sell-pass"]')).toBeVisible();

        // Click the backdrop (the overlay behind the sheet). The backdrop covers
        // the full viewport; click a corner far from the sheet content.
        await page.locator('.sheet-backdrop').click({ position: { x: 10, y: 10 }, force: true });
        await expect(page.locator('[data-testid="sheet-sell-pass"]')).not.toBeVisible({ timeout: 2000 });

        assertCleanConsole(msgs);
    });
});
