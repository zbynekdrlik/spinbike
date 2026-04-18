import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

test.describe('Monthly pass — expired state', () => {
    test('expired pass → red banner, charge buttons return to paid mode', async ({ page, request }) => {
        const consoleMessages = setupConsoleCheck(page);
        const baseURL = 'http://localhost:8099';
        const token = await loginViaAPI(page, baseURL, 'staff@test.com', 'staff123');

        const cardBarcode = 'EXPIRED-PASS-CARD';
        const seedResp = await request.post(`${baseURL}/api/test/seed-expired-pass`, {
            data: { barcode: cardBarcode, valid_until: '2020-01-01' },
            headers: { Authorization: `Bearer ${token}` },
        });
        expect(seedResp.ok()).toBeTruthy();

        await page.goto('/staff');
        const searchInput = page.locator('input[type="search"]');
        await searchInput.focus();
        await page.keyboard.type(cardBarcode, { delay: 30 });
        await page.locator('[data-testid="search-result"]').first().click();

        const banner = page.locator('[data-testid="pass-banner-expired"]');
        await expect(banner).toBeVisible();
        await expect(banner).toContainText('expired');
        await expect(banner).toContainText('days ago');

        await expect(page.locator('[data-testid="log-visit-btn"]')).toHaveCount(0);
        await expect(page.locator('[data-testid="sell-pass-btn"]')).toBeVisible();

        assertCleanConsole(consoleMessages);
    });
});
