import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Staff navigation', () => {
    test('staff user lands on card dashboard at /staff', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff');
        await page.waitForSelector('h1.page-title');
        expect(await page.textContent('h1.page-title')).toBe('Cards — Quick Dashboard');

        // Nav should show Cards and Classes links, no separate Payments.
        const nav = page.locator('.navbar-links');
        await expect(nav.locator('a[href="/staff"]')).toBeVisible();
        await expect(nav.locator('a[href="/staff/classes"]')).toBeVisible();
        await expect(nav.locator('a[href="/staff/payments"]')).toHaveCount(0);
        await expect(nav.locator('a[href="/staff/cards"]')).toHaveCount(0);

        assertCleanConsole(consoleMessages);
    });

    test('weekly classes view is at /staff/classes', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff/classes');
        await page.waitForSelector('h1.page-title');
        expect(await page.textContent('h1.page-title')).toBe('Staff Dashboard');

        // Wait for loading spinner to clear.
        await page.waitForFunction(() => !document.querySelector('.spinner'), { timeout: 10000 });

        const hasCards = (await page.locator('.class-card').count()) > 0;
        const hasEmpty = (await page.locator('.empty-state').count()) > 0;
        expect(hasCards || hasEmpty).toBe(true);

        assertCleanConsole(consoleMessages);
    });
});
