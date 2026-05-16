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
        // Settings moved into the more-sheet in #82.
        await expect(page.locator('[data-testid="nav-settings"]')).toHaveCount(0);
        await expect(page.locator('[data-testid="nav-more"]')).toBeVisible();

        // For staff/admin, the entire top navbar is hidden — the left rail
        // (AdaptiveNav) is the nav, and the top wordmark wasted ~110px on a
        // 1080p laptop. Identity controls live in the More sheet.
        await expect(page.locator('.navbar')).toBeHidden();
        await expect(page.locator('.navbar-links')).toBeHidden();

        // Existing route-tab assertions
        await page.locator('[data-testid="nav-reports"]').click();
        await expect(page).toHaveURL(/\/reports/);
        await page.locator('[data-testid="nav-schedule"]').click();
        await expect(page).toHaveURL(/\/schedule/);
        // Settings now reached via the More sheet (#82).
        await page.locator('[data-testid="nav-more"]').click();
        await expect(page.locator('[data-testid="more-sheet"]')).toBeVisible();
        await expect(page.locator('[data-testid="more-settings"]')).toBeVisible();
        await page.locator('[data-testid="more-settings"]').click();
        await expect(page).toHaveURL(/\/settings/);
        await page.locator('[data-testid="nav-desk"]').click();
        await expect(page).toHaveURL(/\/staff$/);

        // 'More' sheet workflow: open → see username + lang toggle + logout
        await page.locator('[data-testid="nav-more"]').click();
        await expect(page.locator('[data-testid="more-sheet"]')).toBeVisible();
        await expect(page.locator('[data-testid="more-lang-toggle"]')).toBeVisible();
        await expect(page.locator('[data-testid="more-logout"]')).toBeVisible();

        // Logging out from the sheet clears localStorage and calls
        // location.set_href("/"). waitForURL with the default 'load' event
        // is unreliable on CI because the post-logout page rebootstraps WASM.
        // Poll for the cleared token + URL change instead — both flip
        // synchronously inside the click handler before navigation completes.
        await page.locator('[data-testid="more-logout"]').click();
        await page.waitForFunction(
            () => !localStorage.getItem('spinbike_token') && window.location.pathname !== '/staff',
            { timeout: 15000 },
        );

        assertCleanConsole(consoleMessages);
    });

    test('sidebar layout on desktop viewport (brand strip + sidebar)', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await page.setViewportSize({ width: 1280, height: 800 });
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/staff');
        await expect(page.locator('[data-testid="adaptive-nav"]')).toBeVisible();

        // Same chrome on desktop as on phone — the entire top navbar is
        // hidden on staff/admin; AdaptiveNav handles navigation, and the
        // top wordmark is redundant.
        await expect(page.locator('.navbar')).toBeHidden();
        await expect(page.locator('.navbar-links')).toBeHidden();

        await page.locator('[data-testid="nav-reports"]').click();
        await expect(page).toHaveURL(/\/reports/);
        assertCleanConsole(consoleMessages);
    });
});
