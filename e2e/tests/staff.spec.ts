import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Staff navigation', () => {
    test('staff user lands on card dashboard at /staff', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff');
        // The 'Cards — Quick Dashboard' h1 was removed in #32 (v0.13.10). Use
        // the card-search input as the stable landmark instead.
        await page.waitForSelector('input[type="search"]');
        // Body must NOT contain the old h1 text in either language.
        const body = (await page.locator('body').textContent()) ?? '';
        expect(body.toLowerCase()).not.toContain('cards — quick dashboard');
        expect(body.toLowerCase()).not.toContain('karty — rychly prehlad');

        // AdaptiveNav (replaces old Navbar links) shows Desk + Schedule for staff.
        await expect(page.locator('[data-testid="nav-desk"]')).toBeVisible();
        await expect(page.locator('[data-testid="nav-schedule"]')).toBeVisible();
        // Reports is admin-only and must NOT show for staff. Settings moved
        // into the more-sheet in #82 (still admin-only) — staff opening the
        // more-sheet must not see more-settings either.
        await expect(page.locator('[data-testid="nav-reports"]')).toHaveCount(0);
        await page.click('[data-testid="nav-more"]');
        await expect(page.locator('[data-testid="more-settings"]')).toHaveCount(0);

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

        const hasCards = (await page.locator('.list-row').count()) > 0;
        const hasEmpty = (await page.locator('.empty-state').count()) > 0;
        expect(hasCards || hasEmpty).toBe(true);

        assertCleanConsole(consoleMessages);
    });
});
