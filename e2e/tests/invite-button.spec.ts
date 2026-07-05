import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI, createUniqueUser } from './helpers';

const BASE_URL = 'http://localhost:8099';

async function openEditInfoSheet(page: import('@playwright/test').Page, searchTerm: string) {
    await page.goto('/staff');
    const searchInput = page.locator('input[type="search"]');
    await searchInput.waitFor();
    await searchInput.focus();
    await page.keyboard.type(searchTerm, { delay: 30 });
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

    await page
        .locator('[data-testid="action-panel"] button')
        .filter({ hasText: /edit.info|upravit/i })
        .click();
    await expect(page.locator('[data-testid="sheet-edit-info"]')).toBeVisible();
}

test.describe('Staff "Send invite" button in edit-info form (#111)', () => {
    test('disabled for a card with no saved email', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        const user = await createUniqueUser(adminToken, 0, 'NoEmail');
        await openEditInfoSheet(page, user.name);

        const inviteButton = page.locator('[data-testid="user-edit-send-invite"]');
        await expect(inviteButton).toBeVisible();
        await expect(inviteButton).toBeDisabled();

        assertCleanConsole(consoleMessages);
    });

    test('enabled for a card with a saved email; click sends the invite and closes the sheet', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        const suffix = Array.from({ length: 8 }, () =>
            String.fromCharCode(97 + Math.floor(Math.random() * 26)),
        ).join('');
        const email = `hasemail-${suffix}@test.local`;
        const user = await createUniqueUser(adminToken, 0, 'HasEmail', email);
        await openEditInfoSheet(page, user.name);

        const inviteButton = page.locator('[data-testid="user-edit-send-invite"]');
        await expect(inviteButton).toBeVisible();
        await expect(inviteButton).toBeEnabled();

        await inviteButton.click();

        // The sheet closes on invite completion (success or error) — see
        // edit_info_form.rs — specifically so this confirmation is NOT stuck
        // behind the sheet's own full-viewport backdrop blur (z-index above
        // the alert), which would make it invisible to a real user even
        // though it's technically present in the DOM.
        await expect(page.locator('[data-testid="sheet-edit-info"]')).not.toBeVisible({
            timeout: 5000,
        });

        const success = page.locator('.alert-success');
        await expect(success).toBeVisible({ timeout: 10000 });
        await expect(success).toContainText(email);

        assertCleanConsole(consoleMessages);
    });
});
