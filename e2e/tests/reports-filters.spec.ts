import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Reports filters', () => {
    test('expand reveals event-kind chips and search', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/reports');

        await page.locator('[data-testid="filters-bar"]').click();
        await expect(page.locator('[data-testid="filter-event-kind"]')).toBeVisible();
        await expect(page.locator('[data-testid="filter-kind-all"]')).toBeVisible();
        await expect(page.locator('[data-testid="filter-kind-charge"]')).toBeVisible();
        await expect(page.locator('[data-testid="filter-kind-topup"]')).toBeVisible();
        await expect(page.locator('[data-testid="filter-kind-pass"]')).toBeVisible();
        await expect(page.locator('[data-testid="filter-search"]')).toBeVisible();

        assertCleanConsole(consoleMessages);
    });

    test('selecting a kind chip toggles aria-selected', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/reports');

        await page.locator('[data-testid="filters-bar"]').click();

        const allBtn = page.locator('[data-testid="filter-kind-all"]');
        const chargeBtn = page.locator('[data-testid="filter-kind-charge"]');

        await expect(allBtn).toHaveAttribute('aria-selected', 'true');
        await chargeBtn.click();
        await expect(chargeBtn).toHaveAttribute('aria-selected', 'true');
        await expect(allBtn).toHaveAttribute('aria-selected', 'false');

        // Reset returns to "All"
        await page.locator('[data-testid="filters-reset"]').click();
        await expect(allBtn).toHaveAttribute('aria-selected', 'true');

        assertCleanConsole(consoleMessages);
    });

    test('typing in search shows filters-active indicator', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/reports');

        await page.locator('[data-testid="filters-bar"]').click();
        await page.locator('[data-testid="filter-search"]').fill('something');
        await expect(page.locator('[data-testid="filters-active"]')).toBeVisible();

        await page.locator('[data-testid="filters-reset"]').click();
        await expect(page.locator('[data-testid="filters-active"]')).toHaveCount(0);

        assertCleanConsole(consoleMessages);
    });
});
