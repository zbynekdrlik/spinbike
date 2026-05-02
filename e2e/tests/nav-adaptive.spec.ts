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

        // For staff/admin, the top navbar collapses to just the SpinBike
        // brand wordmark — `.navbar-links` (username/Logout/EN-SK) is hidden
        // on every size; the brand stays visible. Identity controls live in
        // the More sheet.
        await expect(page.locator('.navbar-brand')).toBeVisible();
        await expect(page.locator('.navbar-links')).toBeHidden();

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

        // Same chrome split on desktop as on phone: brand wordmark stays
        // in the top navbar, identity controls live in the More sheet.
        await expect(page.locator('.navbar-brand')).toBeVisible();
        await expect(page.locator('.navbar-links')).toBeHidden();

        await page.locator('[data-testid="nav-reports"]').click();
        await expect(page).toHaveURL(/\/reports/);
        assertCleanConsole(consoleMessages);
    });
});
