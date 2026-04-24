import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Reports range', () => {
    test('Week / Month UI modes load without error', async ({ page }) => {
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

    test('API rejects > 93-day range with 400', async ({ page }) => {
        const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        const res = await fetch(`${BASE_URL}/api/reports/range?from=2025-01-01&to=2026-04-25`, {
            headers: { Authorization: `Bearer ${token}` },
        });
        expect(res.status).toBe(400);
    });

    test('API accepts exactly 93-day range with 200', async ({ page }) => {
        const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        const res = await fetch(`${BASE_URL}/api/reports/range?from=2026-01-01&to=2026-04-04`, {
            headers: { Authorization: `Bearer ${token}` },
        });
        expect(res.status).toBe(200);
    });

    test('API rejects to < from with 400', async ({ page }) => {
        const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        const res = await fetch(`${BASE_URL}/api/reports/range?from=2026-04-15&to=2026-04-10`, {
            headers: { Authorization: `Bearer ${token}` },
        });
        expect(res.status).toBe(400);
    });
});
