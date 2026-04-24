import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Reports alerts banner', () => {
    test('banner reflects low-credit card and dismiss persists in localStorage', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        // Seed a card with credit < 5 to trigger the low-credit alert.
        const suffix = `${Date.now()}${Math.random().toString(36).slice(2, 6)}`;
        await fetch(`${BASE_URL}/api/cards/activate`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
            body: JSON.stringify({
                barcode: `LC-${suffix}`,
                initial_credit: 2.0,
                first_name: 'Low',
                last_name: `Credit${suffix}`,
            }),
        });

        // Clear any per-day dismissal from a prior test run on the same day.
        await page.addInitScript(() => {
            try {
                Object.keys(localStorage)
                    .filter((k) => k.startsWith('reports_alerts_dismissed_'))
                    .forEach((k) => localStorage.removeItem(k));
            } catch {
                /* ignore */
            }
        });

        await page.goto('/reports');

        await expect(page.locator('[data-testid="alerts-banner"]')).toBeVisible();
        await expect(page.locator('[data-testid="alert-low-credit"]')).toBeVisible();

        // Dismiss the row; banner row disappears for today.
        await page.locator('[data-testid="alert-low-credit-dismiss"]').click();
        await expect(page.locator('[data-testid="alert-low-credit"]')).toHaveCount(0);

        assertCleanConsole(consoleMessages);
    });
});
