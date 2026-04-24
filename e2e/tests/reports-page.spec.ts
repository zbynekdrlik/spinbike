import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Reports page', () => {
    test('loads with KPI cards, feed, filters', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/reports');

        await expect(page.locator('[data-testid="reports-page"]')).toBeVisible();
        await expect(page.locator('[data-testid="kpi-revenue"]')).toBeVisible();
        await expect(page.locator('[data-testid="kpi-attendance"]')).toBeVisible();
        await expect(page.locator('[data-testid="kpi-passes"]')).toBeVisible();
        await expect(page.locator('[data-testid="kpi-cash-in"]')).toBeVisible();
        await expect(page.locator('[data-testid="filters-bar"]')).toBeVisible();

        assertCleanConsole(consoleMessages);
    });

    test('date prev/next buttons change the label', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/reports');

        const initial = await page.locator('[data-testid="date-label"]').innerText();
        await page.locator('[data-testid="date-prev"]').click();
        const yesterday = await page.locator('[data-testid="date-label"]').innerText();
        expect(yesterday).not.toBe(initial);
        await page.locator('[data-testid="date-next"]').click();
        await expect(page.locator('[data-testid="date-label"]')).toHaveText(initial);

        assertCleanConsole(consoleMessages);
    });

    test('Week/Month range buttons toggle', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/reports');

        await page.locator('[data-testid="range-week"]').click();
        await expect(page.locator('[data-testid="kpi-revenue"]')).toBeVisible();
        await page.locator('[data-testid="range-month"]').click();
        await expect(page.locator('[data-testid="kpi-revenue"]')).toBeVisible();
        await page.locator('[data-testid="range-day"]').click();
        await expect(page.locator('[data-testid="kpi-revenue"]')).toBeVisible();

        assertCleanConsole(consoleMessages);
    });

    test('calendar picker sheet opens and sets anchor', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/reports');

        await page.locator('[data-testid="date-label"]').click();
        await expect(page.locator('[data-testid="sheet-calendar-picker"]')).toBeVisible();
        await page.locator('[data-testid="calendar-picker-input"]').fill('2026-01-15');
        await page.locator('[data-testid="calendar-picker-confirm"]').click();
        await expect(page.locator('[data-testid="date-label"]')).toHaveText('2026-01-15');

        assertCleanConsole(consoleMessages);
    });
});
