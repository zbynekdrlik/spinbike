import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, setEnglishLanguage } from './helpers';

test.describe('Login page — customer login-link form (#109)', () => {
    test('submitting the customer email form shows a confirmation state', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await setEnglishLanguage(page);

        await page.goto('/login');
        await page.waitForSelector('h1.page-title');

        // Password form (admin/staff) stays untouched below.
        await expect(page.locator('form').first()).toBeVisible();

        await expect(page.locator('[data-testid="customer-login-heading"]')).toBeVisible();
        await page.fill('[data-testid="login-link-email"]', 'customer@test.com');
        await page.click('[data-testid="login-link-submit"]');

        await page.waitForSelector('[data-testid="login-link-sent"]', { timeout: 10000 });
        // The form is gone, replaced by the confirmation.
        await expect(page.locator('[data-testid="login-link-form"]')).not.toBeVisible();

        assertCleanConsole(consoleMessages);
    });

    // Regression test for #151: an admin/staff owner who typed their OWN
    // email into this customer-only field got the same generic success
    // banner as a real customer — with no hint why nothing arrived (the
    // backend only sends for role=customer accounts, but always 200s by
    // design to avoid enumeration). The fix is a STATIC hint shown
    // unconditionally (before any submission, regardless of email typed) —
    // asserting it is visible on page load, not derived from any API
    // response, is exactly what proves it adds zero enumeration surface.
    test('a static hint clarifies the login-link field is customer-only (#151)', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await setEnglishLanguage(page);

        await page.goto('/login');
        await page.waitForSelector('h1.page-title');

        const hint = page.locator('[data-testid="login-link-customer-only-help"]');
        await expect(hint).toBeVisible();
        await expect(hint).toHaveText('This link is for client accounts only. Staff and admin log in with a password above.');

        assertCleanConsole(consoleMessages);
    });

    test('customer email form works even for an email that does not exist (no enumeration)', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await setEnglishLanguage(page);

        await page.goto('/login');
        await page.waitForSelector('h1.page-title');

        await page.fill('[data-testid="login-link-email"]', 'no-such-account@test.local');
        await page.click('[data-testid="login-link-submit"]');

        // Server always returns 200 regardless of whether the account
        // exists — the UI shows the same confirmation either way.
        await page.waitForSelector('[data-testid="login-link-sent"]', { timeout: 10000 });

        assertCleanConsole(consoleMessages);
    });

    // Regression test for #152: the button gave no visible signal that a
    // click registered (only a subtle disabled/opacity change on a
    // low-contrast btn--ghost), so users who couldn't tell it worked
    // clicked again — the prod log showed two sends ~2.5 min apart for the
    // same email. A ticket-validator proved live there is no code-level
    // double-submit (a real double-click already fires exactly one
    // request, guarded by `disabled=move || loading.get()`) — the real
    // fix is a clear "sending" loading state, mirroring the staff
    // password button's own loading-text swap (login.rs).
    test('login-link submit shows an immediate loading state, and a rapid double-click still fires exactly one request (#152)', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await setEnglishLanguage(page);

        // Delay the response so the "in flight" state is reliably
        // observable before the form swaps to the success confirmation,
        // and so a rapid double-click's second click still lands while
        // the first request is still pending.
        await page.route('**/api/auth/request-login-link', async (route) => {
            await new Promise((r) => setTimeout(r, 500));
            await route.continue();
        });

        const requestLinkCalls: string[] = [];
        page.on('request', (req) => {
            if (req.url().endsWith('/api/auth/request-login-link') && req.method() === 'POST') {
                requestLinkCalls.push(req.url());
            }
        });

        await page.goto('/login');
        await page.waitForSelector('h1.page-title');

        const submitBtn = page.locator('[data-testid="login-link-submit"]');
        await page.fill('[data-testid="login-link-email"]', 'loading-state@test.com');

        // Two clicks dispatched back-to-back — the second lands while the
        // button is (or should be) already disabled from the first.
        await submitBtn.click();
        await submitBtn.click({ force: true });

        // The button must show a clear "sending" state immediately — well
        // before the artificial 500ms delay resolves.
        await expect(submitBtn).toHaveText('Sending...', { timeout: 1000 });

        await page.waitForSelector('[data-testid="login-link-sent"]', { timeout: 10000 });

        // Regardless of the double-click, exactly one request must fire.
        expect(requestLinkCalls.length).toBe(1);

        assertCleanConsole(consoleMessages);
    });
});
