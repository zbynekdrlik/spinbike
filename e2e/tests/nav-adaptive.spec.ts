import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Adaptive nav', () => {
    test('bottom tabs on mobile viewport (with More sheet)', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await page.setViewportSize({ width: 375, height: 812 });
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/staff');
        await expect(page.locator('[data-testid="adaptive-nav"]')).toBeVisible();
        await expect(page.locator('[data-testid="nav-desk"]')).toBeVisible();
        await expect(page.locator('[data-testid="nav-schedule"]')).toBeVisible();
        await expect(page.locator('[data-testid="nav-reports"]')).toBeVisible();
        await expect(page.locator('[data-testid="nav-settings"]')).toBeVisible();
        await expect(page.locator('[data-testid="nav-more"]')).toBeVisible();

        // Top navbar is hidden on phone for staff/admin (body:has rule).
        await expect(page.locator('.navbar')).toBeHidden();

        // Existing route-tab assertions
        await page.locator('[data-testid="nav-reports"]').click();
        await expect(page).toHaveURL(/\/reports/);
        await page.locator('[data-testid="nav-schedule"]').click();
        await expect(page).toHaveURL(/\/schedule/);
        await page.locator('[data-testid="nav-settings"]').click();
        await expect(page).toHaveURL(/\/settings/);
        await page.locator('[data-testid="nav-desk"]').click();
        await expect(page).toHaveURL(/\/staff$/);

        // 'More' sheet workflow: open → see username + lang toggle + logout
        await page.locator('[data-testid="nav-more"]').click();
        await expect(page.locator('[data-testid="more-sheet"]')).toBeVisible();
        await expect(page.locator('[data-testid="more-lang-toggle"]')).toBeVisible();
        await expect(page.locator('[data-testid="more-logout"]')).toBeVisible();

        // Logging out from the sheet → redirects (post-logout default URL).
        await page.locator('[data-testid="more-logout"]').click();
        await page.waitForURL(/\/(login)?$/, { timeout: 5000 });

        assertCleanConsole(consoleMessages);
    });

    test('sidebar layout on desktop viewport (top navbar still visible)', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await page.setViewportSize({ width: 1280, height: 800 });
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/staff');
        await expect(page.locator('[data-testid="adaptive-nav"]')).toBeVisible();

        // On desktop, the top navbar IS visible (the body:has hide rule
        // is gated by max-width: 540px).
        await expect(page.locator('.navbar')).toBeVisible();

        await page.locator('[data-testid="nav-reports"]').click();
        await expect(page).toHaveURL(/\/reports/);
        assertCleanConsole(consoleMessages);
    });
});
