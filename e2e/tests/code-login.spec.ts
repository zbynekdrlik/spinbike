import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole, setEnglishLanguage } from './helpers';

const BASE_URL = 'http://localhost:8099';

// #227 — 6-digit email login code. The end-to-end flow that closes the iOS
// installed-PWA logged-out loop: a customer enters their email, gets a code, and
// logs in entirely inside the app (no magic link / Safari hop). The raw code is
// obtained via the test-only /api/test/mint-login-code seam (the public
// request-login-code endpoint never echoes it — no enumeration).
test.describe('Login page — customer login-code (#227)', () => {
    async function seedCustomer(page: import('@playwright/test').Page): Promise<string> {
        const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        const suffix = Array.from({ length: 8 }, () =>
            String.fromCharCode(97 + Math.floor(Math.random() * 26)),
        ).join('');
        const email = `code-${suffix}@test.local`;
        const resp = await fetch(`${BASE_URL}/api/users`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
            body: JSON.stringify({ name: `Code ${suffix}`, email, card_code: `CL-${suffix}` }),
        });
        if (!resp.ok) throw new Error(`create user failed: ${resp.status} ${await resp.text()}`);
        // loginViaAPI stored the ADMIN session in localStorage; clear it so the
        // code-login flow below runs as an anonymous user (the real scenario).
        await page.evaluate(() => {
            localStorage.removeItem('spinbike_token');
            localStorage.removeItem('spinbike_user');
        });
        return email;
    }

    test('the code toggle reveals the code form; a valid code logs the customer in', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);
        const email = await seedCustomer(page);

        await setEnglishLanguage(page);
        await page.goto('/login');
        await page.waitForSelector('h1.page-title');

        // The email-link form is the DEFAULT method (toggle unchanged behaviour).
        await expect(page.locator('[data-testid="login-link-form"]')).toBeVisible();

        // Switch to the code method.
        await page.click('[data-testid="login-method-code"]');
        await expect(page.locator('[data-testid="code-login-email"]')).toBeVisible();

        // Step 1 — request a code (the real endpoint; response is a uniform 200).
        await page.fill('[data-testid="code-login-email"]', email);
        await page.click('[data-testid="code-login-send"]');
        await page.waitForSelector('[data-testid="code-login-sent"]', { timeout: 10000 });

        // The code input offers the OS one-time-code keyboard suggestion on iOS.
        const codeInput = page.locator('[data-testid="code-login-code"]');
        await expect(codeInput).toHaveAttribute('inputmode', 'numeric');
        await expect(codeInput).toHaveAttribute('autocomplete', 'one-time-code');

        // Mint a known-valid code for this customer (test-only seam).
        const mintResp = await fetch(`${BASE_URL}/api/test/mint-login-code`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ email }),
        });
        if (!mintResp.ok) throw new Error(`mint failed: ${mintResp.status} ${await mintResp.text()}`);
        const { code } = await mintResp.json();
        expect(code).toMatch(/^\d{6}$/);

        // Step 2 — enter the code and log in.
        await codeInput.fill(code);
        await page.click('[data-testid="code-login-submit"]');

        // Logged in → redirected to the customer balance page with a stored session.
        await page.waitForURL('**/my/balance', { timeout: 10000 });
        const token = await page.evaluate(() => localStorage.getItem('spinbike_token'));
        expect(token).toBeTruthy();
        await expect(page.locator('[data-testid="door-open-button"]')).toBeVisible({ timeout: 10000 });

        assertCleanConsole(consoleMessages);
    });

    test('a wrong code shows a localized error and does not log in', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        const email = await seedCustomer(page);

        await setEnglishLanguage(page);
        await page.goto('/login');
        await page.waitForSelector('h1.page-title');

        await page.click('[data-testid="login-method-code"]');
        await page.fill('[data-testid="code-login-email"]', email);
        await page.click('[data-testid="code-login-send"]');
        await page.waitForSelector('[data-testid="code-login-sent"]', { timeout: 10000 });

        // A definitely-wrong code → uniform rejection banner, no redirect.
        await page.fill('[data-testid="code-login-code"]', '000001');
        await page.click('[data-testid="code-login-submit"]');

        const err = page.locator('[data-testid="code-login-error"]');
        await expect(err).toBeVisible({ timeout: 10000 });
        await expect(err).toHaveText('The code is wrong or has expired');
        // Still on /login (no session stored, no redirect).
        await expect(page).toHaveURL(/\/login$/);
        const token = await page.evaluate(() => localStorage.getItem('spinbike_token'));
        expect(token).toBeFalsy();

        // The 401 is an expected outcome (helpers filter 4xx) — console stays clean.
        assertCleanConsole(consoleMessages);
    });
});
