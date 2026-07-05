import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, setEnglishLanguage } from './helpers';

test.describe('Login page — customer login-link form (#109)', () => {
    test('submitting the customer email form shows a confirmation state', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await setEnglishLanguage(page);

        await page.goto('/login');
        await page.waitForSelector('h1.page-title');

        // Password form (admin/staff) stays untouched below.
        await expect(page.locator('form').first()).toBeVisible();

        await expect(page.locator('[data-testid="customer-login-heading"]')).toBeVisible();
        await page.fill('[data-testid="login-link-email"]', 'customer@test.com');
        await page.click('[data-testid="login-link-submit"]');

        await page.waitForSelector('[data-testid="login-link-sent"]', { timeout: 10000 });
        // The form is gone, replaced by the confirmation.
        await expect(page.locator('[data-testid="login-link-form"]')).not.toBeVisible();

        assertCleanConsole(consoleMessages);
    });

    test('customer email form works even for an email that does not exist (no enumeration)', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await setEnglishLanguage(page);

        await page.goto('/login');
        await page.waitForSelector('h1.page-title');

        await page.fill('[data-testid="login-link-email"]', 'no-such-account@test.local');
        await page.click('[data-testid="login-link-submit"]');

        // Server always returns 200 regardless of whether the account
        // exists — the UI shows the same confirmation either way.
        await page.waitForSelector('[data-testid="login-link-sent"]', { timeout: 10000 });

        assertCleanConsole(consoleMessages);
    });
});
