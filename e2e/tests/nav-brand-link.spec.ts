import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('SpinBike brand link — role-aware target + active state', () => {
    test('staff click on SpinBike → /staff with Desk tab active', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/schedule');
        await page.locator('[data-testid="brand-link"]').click();

        await expect(page).toHaveURL(/\/staff$/);
        await expect(page.locator('[data-testid="nav-desk"]')).toHaveAttribute('aria-current', 'page');

        // Settings moved into the more-sheet in #82, so it is no longer a
        // direct nav item.
        for (const id of ['nav-schedule', 'nav-reports']) {
            const el = page.locator(`[data-testid="${id}"]`);
            if (await el.count() > 0) {
                await expect(el).toHaveAttribute('aria-current', 'false');
            }
        }
        assertCleanConsole(msgs);
    });

    test('staff visiting / is redirected to /staff', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/');
        await expect(page).toHaveURL(/\/staff$/);
        await expect(page.locator('[data-testid="nav-desk"]')).toHaveAttribute('aria-current', 'page');
        assertCleanConsole(msgs);
    });

    test('logged-out / renders the public schedule, no adaptive nav', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        await page.goto('/');
        await expect(page.locator('[data-testid="brand-link"]')).toBeVisible();
        await expect(page.locator('[data-testid="adaptive-nav"]')).toHaveCount(0);
        assertCleanConsole(msgs);
    });
});
