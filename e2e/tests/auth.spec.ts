import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, setEnglishLanguage, passwordLoginForm } from './helpers';

test.describe('Authentication flows', () => {
    test.beforeEach(async ({ page }) => {
        await setEnglishLanguage(page);
    });

    // NOTE: public self-registration was removed server-side in #108, and the
    // `/register` page + all nav/login links were removed client-side in #112
    // (invite-only onboarding). The former "register a new user via UI" test
    // is gone with the feature; onboarding is now covered by the invite →
    // /welcome flow (tested under #109). Logout coverage below bootstraps via
    // login instead.

    test('logout and verify nav reverts', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        // Bootstrap a session by logging in as the seeded customer (public
        // register is gone in #108).
        await page.goto('/login');
        await page.waitForSelector('h1.page-title');
        // /login now has a SECOND type="email" input + submit button (the
        // customer login-link section, #109) below this password form —
        // passwordLoginForm() scopes to the form that has a password input.
        const form1 = passwordLoginForm(page);
        await form1.locator('input[type="email"]').fill('customer@test.com');
        await form1.locator('input[type="password"]').fill('password123');
        await form1.locator('button[type="submit"]').click();
        await page.waitForURL('/', { timeout: 10000 });

        // Verify logged in
        const nav = page.locator('.navbar-links');
        await expect(nav.locator('.navbar-user')).toContainText('Test Customer');

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

        // After reload with no token, nav shows Login
        const navAfter = page.locator('.navbar-links');
        await expect(navAfter.locator('a[href="/login"]')).toBeVisible({ timeout: 10000 });
        // Public registration was removed (#112, invite-only) — the register
        // link must be gone from the logged-out nav.
        await expect(navAfter.locator('a[href="/register"]')).not.toBeVisible();

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

        // /login now has a SECOND type="email" input + submit button (the
        // customer login-link section, #109) below this password form —
        // passwordLoginForm() scopes to the form that has a password input.
        const form2 = passwordLoginForm(page);
        await form2.locator('input[type="email"]').fill('customer@test.com');
        await form2.locator('input[type="password"]').fill('password123');
        await form2.locator('button[type="submit"]').click();

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

        const form3 = passwordLoginForm(page);
        await form3.locator('input[type="email"]').fill('customer@test.com');
        await form3.locator('input[type="password"]').fill('wrongpassword');
        await form3.locator('button[type="submit"]').click();

        // Should show error, not redirect
        await page.waitForSelector('.alert.alert-error', { timeout: 5000 });
        const errorText = await page.textContent('.alert.alert-error');
        expect(errorText).toBeTruthy();

        // Should still be on /login
        expect(page.url()).toContain('/login');

        assertCleanConsole(consoleMessages);
    });

    test('/register no longer renders the registration form (#112)', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await page.goto('/register');
        await page.waitForSelector('.page');

        // The server's SPA static fallback still serves index.html (200) for
        // any dotless path — see ci-deploy skill — but the client-side router
        // no longer has a /register <Route>, so its own `fallback` renders
        // the "page not found" message instead of RegisterPage.
        await expect(page.locator('.page')).toContainText(/not found/i);
        await expect(page.locator('h1.page-title', { hasText: 'Register' })).toHaveCount(0);
        await expect(page.locator('input[type="password"][minlength="6"]')).toHaveCount(0);

        assertCleanConsole(consoleMessages);
    });
});
