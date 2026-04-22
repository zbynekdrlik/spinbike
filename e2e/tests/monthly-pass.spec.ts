import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Monthly pass — sell, banner, visit', () => {
    test('sell pass → banner appears → visit logs 0 EUR row', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff');

        // Pick the first card from the seeded test set
        const searchInput = page.locator('input[type="search"]');
        await searchInput.waitFor();
        await searchInput.focus();
        await page.keyboard.type('TestCorp', { delay: 30 });
        await page.locator('[data-testid="search-result"]').first().click();

        // Top up so the card has credit for the pass
        // Jana starts with 50.00 initial credit; after +50 topup → 100.00
        await page.locator('[data-testid="topup-50"]').click();
        await expect(page.locator('[data-testid="card-credit"]')).toContainText('100.00');

        // Open the sell-pass modal
        await page.locator('[data-testid="sell-pass-btn"]').click();
        const modal = page.locator('[data-testid="sheet-sell-pass"]');
        await expect(modal).toBeVisible();

        // Default price should be 35, date should be today + 30
        const priceInput = page.locator('[data-testid="sell-pass-price"]');
        await expect(priceInput).toHaveValue('35.00');

        // Confirm
        await page.locator('[data-testid="sell-pass-confirm"]').click();
        await expect(modal).not.toBeVisible();

        // Banner appears
        const banner = page.locator('[data-testid="pass-banner-active"]');
        await expect(banner).toBeVisible();
        await expect(banner).toContainText('Monthly pass valid until');
        await expect(banner).toContainText('days remaining');

        // Credit dropped by 35: 100.00 - 35.00 = 65.00
        await expect(page.locator('[data-testid="card-credit"]')).toContainText('65.00');

        // Charge buttons are now "Log visit" buttons
        const visitBtn = page.locator('[data-testid="log-visit-btn"]').first();
        await expect(visitBtn).toBeVisible();
        await visitBtn.click();

        // History shows a visit row with 0.00 amount
        await expect(page.locator('.txn-row-visit')).toContainText('visit');
        await expect(page.locator('.txn-row-visit')).toContainText('0.00');

        assertCleanConsole(consoleMessages);
    });
});
