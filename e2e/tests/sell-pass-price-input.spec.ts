import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

// Regression: the sell-pass price input used a controlled prop:value with
// `format!("{:.2}", price)` so every keystroke reformatted the field back to
// the current f64 with 2 decimals, making it impossible to type a custom
// price char-by-char (trailing digits lost, caret jumped). The fix switched
// the input to uncontrolled (node_ref + read on submit) with a text/decimal
// inputmode for better mobile UX.
test.describe('Monthly pass — price input UX', () => {
    test('typing a custom price char-by-char survives and is charged', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff');

        // Pick a test card and top it up so it has enough credit (80 EUR) to
        // afford any price we might type below.
        const searchInput = page.locator('input[type="search"]');
        await searchInput.waitFor();
        await searchInput.focus();
        await page.keyboard.type('TestCorp', { delay: 30 });
        await page.locator('[data-testid="search-result"]').first().click();
        await page.locator('[data-testid="topup-30"]').click();
        await expect(page.locator('[data-testid="card-credit"]')).toContainText('80.00');

        // Open the sell-pass modal.
        await page.locator('[data-testid="sell-pass-btn"]').click();
        const modal = page.locator('[data-testid="sheet-sell-pass"]');
        await expect(modal).toBeVisible();

        // Default price is 35.00 (seeded service).
        const priceInput = page.locator('[data-testid="sell-pass-price"]');
        await expect(priceInput).toHaveValue('35.00');

        // Clear and type "40" character-by-character with a small delay to
        // mimic a real user pressing keys. This exercises the per-keystroke
        // round-trip that the old controlled input broke.
        await priceInput.focus();
        await priceInput.press('ControlOrMeta+a');
        await priceInput.press('Delete');
        await page.keyboard.type('40', { delay: 50 });

        // The typed value must survive — not snap back to the default.
        await expect(priceInput).toHaveValue('40');

        // Confirm: credit drops by the typed amount, not by the default 35.
        await page.locator('[data-testid="sell-pass-confirm"]').click();
        await expect(modal).not.toBeVisible();
        await expect(page.locator('[data-testid="pass-banner-active"]')).toBeVisible();
        // 80.00 - 40.00 = 40.00
        await expect(page.locator('[data-testid="card-credit"]')).toContainText('40.00');

        assertCleanConsole(consoleMessages);
    });

    test('comma decimal separator is accepted', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff');

        const searchInput = page.locator('input[type="search"]');
        await searchInput.waitFor();
        await searchInput.focus();
        // Use a different seeded card to avoid state leakage between tests.
        await page.keyboard.type('Vzorny', { delay: 30 });
        await page.locator('[data-testid="search-result"]').first().click();
        await page.locator('[data-testid="topup-30"]').click();

        await page.locator('[data-testid="sell-pass-btn"]').click();
        const priceInput = page.locator('[data-testid="sell-pass-price"]');
        await priceInput.focus();
        await priceInput.press('ControlOrMeta+a');
        await priceInput.press('Delete');
        // Slovak keyboard — comma decimal separator.
        await page.keyboard.type('25,50', { delay: 50 });
        await expect(priceInput).toHaveValue('25,50');

        await page.locator('[data-testid="sell-pass-confirm"]').click();
        await expect(page.locator('[data-testid="sheet-sell-pass"]')).not.toBeVisible();
        await expect(page.locator('[data-testid="pass-banner-active"]')).toBeVisible();

        assertCleanConsole(consoleMessages);
    });
});
