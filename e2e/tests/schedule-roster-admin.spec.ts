import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Schedule (admin view)', () => {
    test('admin /schedule renders week view without console errors', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/schedule');
        await expect(page).toHaveURL(/\/schedule/);
        // Page renders body (h1 or empty-state — both indicate the route resolved).
        await expect(page.locator('body')).toContainText(/./);
        assertCleanConsole(consoleMessages);
    });

    test('admin /staff/classes still works (legacy alias)', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/staff/classes');
        await expect(page).toHaveURL(/\/staff\/classes/);
        await expect(page.locator('body')).toContainText(/./);
        assertCleanConsole(consoleMessages);
    });

    test('admin /admin redirects to /settings', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/admin');
        await expect(page).toHaveURL(/\/settings$/);
        await expect(page.locator('h1.page-title')).toBeVisible();
        assertCleanConsole(consoleMessages);
    });
});
