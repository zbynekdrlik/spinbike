import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

async function createUser(
    adminToken: string,
    prefix: string,
    withEmail: boolean,
): Promise<{ user_id: number; name: string; email: string | null }> {
    const suffix = Array.from({ length: 8 }, () =>
        String.fromCharCode(97 + Math.floor(Math.random() * 26)),
    ).join('');
    const name = `${prefix} ${suffix}`;
    const email = withEmail ? `${prefix.toLowerCase()}-${suffix}@test.local` : null;
    const body: Record<string, unknown> = { name, card_code: `${prefix}-${suffix}` };
    if (email) body.email = email;

    const resp = await fetch(`${BASE_URL}/api/users`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
        body: JSON.stringify(body),
    });
    if (!resp.ok) {
        throw new Error(`createUser failed: ${resp.status} ${await resp.text()}`);
    }
    const json = await resp.json();
    return { user_id: json.id as number, name, email };
}

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

        const user = await createUser(adminToken, 'NoEmail', false);
        await openEditInfoSheet(page, user.name);

        const inviteButton = page.locator('[data-testid="user-edit-send-invite"]');
        await expect(inviteButton).toBeVisible();
        await expect(inviteButton).toBeDisabled();

        assertCleanConsole(consoleMessages);
    });

    test('enabled for a card with a saved email; click sends the invite', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        const user = await createUser(adminToken, 'HasEmail', true);
        await openEditInfoSheet(page, user.name);

        const inviteButton = page.locator('[data-testid="user-edit-send-invite"]');
        await expect(inviteButton).toBeVisible();
        await expect(inviteButton).toBeEnabled();

        await inviteButton.click();

        const success = page.locator('.alert-success');
        await expect(success).toBeVisible({ timeout: 10000 });
        await expect(success).toContainText(user.email as string);

        assertCleanConsole(consoleMessages);
    });
});
