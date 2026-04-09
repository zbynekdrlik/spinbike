import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Staff pages', () => {
    test('staff user can access staff dashboard', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff');
        await page.waitForSelector('h1.page-title');
        expect(await page.textContent('h1.page-title')).toBe('Staff Dashboard');

        // Nav should show Staff, Cards, Payments links
        const nav = page.locator('.navbar-links');
        await expect(nav.locator('a[href="/staff"]')).toBeVisible();
        await expect(nav.locator('a[href="/staff/cards"]')).toBeVisible();
        await expect(nav.locator('a[href="/staff/payments"]')).toBeVisible();

        assertCleanConsole(consoleMessages);
    });

    test('staff dashboard shows classes for the week', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff');
        await page.waitForSelector('h1.page-title');

        // Wait for loading to finish
        await page.waitForFunction(() => {
            return !document.querySelector('.spinner');
        }, { timeout: 10000 });

        // Should have class cards or "No classes this week" message
        const classCards = page.locator('.class-card');
        const emptyState = page.locator('.empty-state');
        const hasCards = (await classCards.count()) > 0;
        const hasEmpty = (await emptyState.count()) > 0;
        expect(hasCards || hasEmpty).toBe(true);

        assertCleanConsole(consoleMessages);
    });

    test('card lookup by barcode', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff/cards');
        await page.waitForSelector('h1.page-title');
        expect(await page.textContent('h1.page-title')).toBe('Card Operations');

        // Fill barcode input (placeholder="Enter barcode")
        const barcodeInput = page.locator('input[placeholder="Enter barcode"]');
        await expect(barcodeInput).toBeVisible();
        await barcodeInput.fill('70701001');

        // Click Lookup button
        await page.click('button:has-text("Lookup")');

        // Wait for card info to appear
        await page.waitForSelector('.card', { timeout: 5000 });

        // Verify card info is displayed (barcode and credit)
        const cardContent = await page.textContent('.card');
        expect(cardContent).toContain('70701001');
        expect(cardContent).toContain('CZK');

        assertCleanConsole(consoleMessages);
    });

    test('payments page: lookup card and charge', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff/payments');
        await page.waitForSelector('h1.page-title');
        expect(await page.textContent('h1.page-title')).toBe('Payments');

        // Lookup the test card
        const barcodeInput = page.locator('input[placeholder="Scan barcode"]');
        await expect(barcodeInput).toBeVisible();
        await barcodeInput.fill('70701001');
        await page.click('button:has-text("Lookup")');

        // Wait for card info to display
        await page.waitForSelector('.card', { timeout: 5000 });
        const cardInfo = await page.textContent('.card');
        expect(cardInfo).toContain('70701001');
        expect(cardInfo).toContain('CZK');

        // The charge form should appear with service select and amount
        const chargeButton = page.locator('button:has-text("Charge")');
        await expect(chargeButton).toBeVisible();

        // Fill in amount and charge
        const amountInputs = page.locator('input[type="number"]');
        // First number input in the charge form
        await amountInputs.first().fill('10');
        await chargeButton.click();

        // Wait for success message
        await page.waitForSelector('.alert-success', { timeout: 5000 });
        const successText = await page.textContent('.alert-success');
        expect(successText).toContain('Charged');

        assertCleanConsole(consoleMessages);
    });
});
