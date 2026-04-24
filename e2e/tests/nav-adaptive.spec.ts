import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Adaptive nav', () => {
    test('bottom tabs on mobile viewport', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await page.setViewportSize({ width: 375, height: 812 });
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/staff');
        await expect(page.locator('[data-testid="adaptive-nav"]')).toBeVisible();
        await expect(page.locator('[data-testid="nav-desk"]')).toBeVisible();
        await expect(page.locator('[data-testid="nav-schedule"]')).toBeVisible();
        await expect(page.locator('[data-testid="nav-reports"]')).toBeVisible();
        await expect(page.locator('[data-testid="nav-settings"]')).toBeVisible();

        await page.locator('[data-testid="nav-reports"]').click();
        await expect(page).toHaveURL(/\/reports/);
        await page.locator('[data-testid="nav-schedule"]').click();
        await expect(page).toHaveURL(/\/schedule/);
        await page.locator('[data-testid="nav-settings"]').click();
        await expect(page).toHaveURL(/\/settings/);
        await page.locator('[data-testid="nav-desk"]').click();
        await expect(page).toHaveURL(/\/staff$/);

        assertCleanConsole(consoleMessages);
    });

    test('sidebar layout on desktop viewport', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await page.setViewportSize({ width: 1280, height: 800 });
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/staff');
        await expect(page.locator('[data-testid="adaptive-nav"]')).toBeVisible();
        await page.locator('[data-testid="nav-reports"]').click();
        await expect(page).toHaveURL(/\/reports/);
        assertCleanConsole(consoleMessages);
    });
});
