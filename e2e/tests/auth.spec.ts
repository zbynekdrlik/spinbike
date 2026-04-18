import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, setEnglishLanguage } from './helpers';

test.describe('Authentication flows', () => {
    test.beforeEach(async ({ page }) => {
        await setEnglishLanguage(page);
    });

    test('register a new user via UI', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await page.goto('/register');
        await page.waitForSelector('h1.page-title');
        expect(await page.textContent('h1.page-title')).toBe('Register');

        // Fill the registration form
        // Name field is first text input
        const nameInput = page.locator('input[type="text"]').first();
        await nameInput.fill('New User');
        await page.fill('input[type="email"]', 'newuser@test.com');
        await page.fill('input[type="password"]', 'newpass123');
        await page.click('button[type="submit"]');

        // After register, app redirects to /
        await page.waitForURL('/', { timeout: 10000 });

        // Verify logged-in state: nav should show "My Bookings" and "Balance"
        const nav = page.locator('.navbar-links');
        await expect(nav.locator('a[href="/my/bookings"]')).toBeVisible();
        await expect(nav.locator('a[href="/my/balance"]')).toBeVisible();

        // Verify user name appears in nav
        await expect(nav.locator('.navbar-user')).toContainText('New User');

        assertCleanConsole(consoleMessages);
    });

    test('logout and verify nav reverts', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        // First register/login to get a session
        await page.goto('/register');
        await page.waitForSelector('h1.page-title');
        const nameInput = page.locator('input[type="text"]').first();
        await nameInput.fill('Logout Tester');
        await page.fill('input[type="email"]', 'logout-tester@test.com');
        await page.fill('input[type="password"]', 'logout123');
        await page.click('button[type="submit"]');
        await page.waitForURL('/', { timeout: 10000 });

        // Verify logged in
        const nav = page.locator('.navbar-links');
        await expect(nav.locator('.navbar-user')).toContainText('Logout Tester');

        // Click Logout button (clears localStorage, bumps auth signal, then navigates to /)
        await nav.locator('button', { hasText: 'Logout' }).click();

        // The logout handler clears localStorage and does location.set_href("/").
        // The navigation to "/" while already on "/" may not fully reload in all browsers.
        // Force a page reload to ensure WASM re-initializes with cleared localStorage.
        await page.waitForTimeout(500);
        await page.reload();
        await page.waitForSelector('.navbar-links', { timeout: 10000 });

        // Verify localStorage was cleared
        const token = await page.evaluate(() => localStorage.getItem('spinbike_token'));
        expect(token).toBeNull();

        // After reload with no token, nav shows Login/Register
        const navAfter = page.locator('.navbar-links');
        await expect(navAfter.locator('a[href="/login"]')).toBeVisible({ timeout: 10000 });
        await expect(navAfter.locator('a[href="/register"]')).toBeVisible();

        // My Bookings link should not be visible
        await expect(navAfter.locator('a[href="/my/bookings"]')).not.toBeVisible();

        assertCleanConsole(consoleMessages);
    });

    test('login with existing user via UI', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        // Use the customer@test.com user created by global setup
        await page.goto('/login');
        await page.waitForSelector('h1.page-title');
        expect(await page.textContent('h1.page-title')).toBe('Login');

        await page.fill('input[type="email"]', 'customer@test.com');
        await page.fill('input[type="password"]', 'password123');
        await page.click('button[type="submit"]');

        await page.waitForURL('/', { timeout: 10000 });

        // Verify logged-in state
        const nav = page.locator('.navbar-links');
        await expect(nav.locator('.navbar-user')).toContainText('Test Customer');
        await expect(nav.locator('a[href="/my/bookings"]')).toBeVisible();

        assertCleanConsole(consoleMessages);
    });

    test('login with wrong password shows error', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await page.goto('/login');
        await page.waitForSelector('h1.page-title');

        await page.fill('input[type="email"]', 'customer@test.com');
        await page.fill('input[type="password"]', 'wrongpassword');
        await page.click('button[type="submit"]');

        // Should show error, not redirect
        await page.waitForSelector('.alert.alert-error', { timeout: 5000 });
        const errorText = await page.textContent('.alert.alert-error');
        expect(errorText).toBeTruthy();

        // Should still be on /login
        expect(page.url()).toContain('/login');

        assertCleanConsole(consoleMessages);
    });

    test('register link navigates from login page', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await page.goto('/login');
        await page.waitForSelector('h1.page-title');

        // Click "Register" link
        await page.click('a[href="/register"]');
        await page.waitForSelector('h1.page-title');
        expect(await page.textContent('h1.page-title')).toBe('Register');

        assertCleanConsole(consoleMessages);
    });
});
