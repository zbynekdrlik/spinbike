import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Admin pages', () => {
    test('admin user can access admin panel', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/admin');
        await page.waitForSelector('h1.page-title');
        expect(await page.textContent('h1.page-title')).toBe('Admin');

        // Nav should show Admin link
        const nav = page.locator('.navbar-links');
        await expect(nav.locator('a[href="/admin"]')).toBeVisible();

        assertCleanConsole(consoleMessages);
    });

    test('admin can view templates tab', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/admin');
        await page.waitForSelector('h1.page-title');

        // Templates tab should be active by default
        const activeTab = page.locator('.seg__item[aria-selected="true"]');
        await expect(activeTab).toContainText(/templates/i);

        // Wait for templates to load
        await page.waitForFunction(() => {
            return !document.querySelector('.spinner');
        }, { timeout: 10000 });

        // Should show template data in a table
        const table = page.locator('table');
        await expect(table).toBeVisible();

        assertCleanConsole(consoleMessages);
    });

    test('admin can create an instructor', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/admin');
        await page.waitForSelector('h1.page-title');

        // Click instructors tab using locator
        await page.locator('[data-testid="admin-tab-instructors"]').click();

        // Wait for data to load
        await page.waitForFunction(() => {
            return !document.querySelector('.spinner');
        }, { timeout: 10000 });

        // Fill in the instructor name
        const nameInput = page.locator('.inline-form input[type="text"]');
        await expect(nameInput).toBeVisible();
        await nameInput.fill('Pavel');

        // Click "Add Instructor"
        await page.locator('button', { hasText: 'Add Instructor' }).click();

        // Wait for the list to refresh
        await page.waitForTimeout(1000);
        await page.waitForFunction(() => {
            return !document.querySelector('.spinner');
        }, { timeout: 10000 });

        // Verify the new instructor appears in the table
        const tableContent = await page.textContent('table');
        expect(tableContent).toContain('Pavel');

        assertCleanConsole(consoleMessages);
    });

    test('admin can switch between tabs', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/admin');
        await page.waitForSelector('h1.page-title');

        // Verify all tab buttons are present
        const tabs = page.locator('.seg__item');
        const tabCount = await tabs.count();
        expect(tabCount).toBe(5); // templates, instructors, services, users, settings

        // Click services tab
        await page.locator('[data-testid="admin-tab-services"]').click();
        await page.waitForFunction(() => {
            return !document.querySelector('.spinner');
        }, { timeout: 10000 });

        // Verify services tab shows (check for price field in the form)
        const priceInput = page.locator('input[type="number"]');
        await expect(priceInput.first()).toBeVisible();

        // Click users tab
        await page.locator('[data-testid="admin-tab-users"]').click();
        await page.waitForFunction(() => {
            return !document.querySelector('.spinner');
        }, { timeout: 10000 });

        // Should show user cards with role selects
        const roleSelect = page.locator('select.form-control');
        // At least the seeded users should be there
        expect(await roleSelect.count()).toBeGreaterThan(0);

        // Click settings tab
        await page.locator('[data-testid="admin-tab-settings"]').click();
        await page.waitForFunction(() => {
            return !document.querySelector('.spinner');
        }, { timeout: 10000 });

        // Settings form should have key/value inputs
        const keyInput = page.locator('.inline-form input[type="text"]').first();
        await expect(keyInput).toBeVisible();

        assertCleanConsole(consoleMessages);
    });

    test('admin can create a service', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/admin');
        await page.waitForSelector('h1.page-title');

        // Click services tab
        await page.locator('[data-testid="admin-tab-services"]').click();
        await page.waitForFunction(() => {
            return !document.querySelector('.spinner');
        }, { timeout: 10000 });

        // Fill service form
        const nameInput = page.locator('.inline-form input[type="text"]');
        await nameInput.fill('Pilates');

        const priceInput = page.locator('.inline-form input[type="number"]');
        await priceInput.fill('150');

        // Click Add Service
        await page.locator('button', { hasText: 'Add Service' }).click();

        // Wait for list to refresh
        await page.waitForTimeout(1000);
        await page.waitForFunction(() => {
            return !document.querySelector('.spinner');
        }, { timeout: 10000 });

        // Verify service appears
        const tableContent = await page.textContent('table');
        expect(tableContent).toContain('Pilates');

        assertCleanConsole(consoleMessages);
    });
});
