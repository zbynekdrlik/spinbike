import { test, expect, devices } from '@playwright/test';
import {
    loginViaAPI,
    setupConsoleCheck,
    assertCleanConsole,
    setEnglishLanguage,
    setIosStandalone,
} from './helpers';

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

// #228 — a magic link is a dead end when running installed standalone on iOS
// (storage is partitioned from Safari, so the link always reopens there
// instead of completing login inside the installed app). `CustomerLoginMethods`
// must therefore LEAD with the code method there — on `/login`'s customer
// section AND on `/welcome`'s invalid-token fallback (same shared component).
const iPhone = devices['iPhone 13'];
test.describe('Customer login method ordering — installed standalone iOS (#228)', () => {
    test.use({
        userAgent: iPhone.userAgent,
        viewport: iPhone.viewport,
        isMobile: iPhone.isMobile,
        hasTouch: iPhone.hasTouch,
    });

    test('standalone + iOS leads with the code form on /login', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await setIosStandalone(page);
        await setEnglishLanguage(page);
        await page.goto('/login');
        await page.waitForSelector('h1.page-title');

        await expect(page.locator('[data-testid="login-method-code"]')).toHaveAttribute(
            'aria-selected',
            'true',
        );
        await expect(page.locator('[data-testid="login-method-link"]')).toHaveAttribute(
            'aria-selected',
            'false',
        );
        await expect(page.locator('[data-testid="code-login-email-form"]')).toBeVisible();
        await expect(page.locator('[data-testid="login-link-form"]')).toHaveCount(0);

        assertCleanConsole(consoleMessages);
    });

    test('standalone + iOS leads with the code form on /welcome invalid-token fallback', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);
        await setIosStandalone(page);
        await setEnglishLanguage(page);
        await page.goto('/welcome');
        await page.waitForSelector('[data-testid="welcome-invalid"]', { timeout: 10000 });

        await expect(page.locator('[data-testid="login-method-code"]')).toHaveAttribute(
            'aria-selected',
            'true',
        );
        await expect(page.locator('[data-testid="code-login-email-form"]')).toBeVisible();

        assertCleanConsole(consoleMessages);
    });

    test('iOS in a plain Safari tab (NOT installed/standalone) still leads with the link form', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);
        // Deliberately no setIosStandalone() — a normal browser tab, not
        // installed. The link method must stay the default here; only the
        // installed-standalone case reorders.
        await setEnglishLanguage(page);
        await page.goto('/login');
        await page.waitForSelector('h1.page-title');

        await expect(page.locator('[data-testid="login-method-link"]')).toHaveAttribute(
            'aria-selected',
            'true',
        );
        await expect(page.locator('[data-testid="login-link-form"]')).toBeVisible();

        assertCleanConsole(consoleMessages);
    });
});

// Android/Chromium is explicitly UNCHANGED (#228): the browser and the
// installed PWA share storage there, so no logged-out loop exists and the
// link stays primary. `setIosStandalone` is applied here too (it only sets
// `navigator.standalone`, an iOS-Safari-only flag with no real meaning on
// Android) specifically to prove the REORDER is gated by the iOS user-agent
// check, not merely by the standalone flag alone.
test.describe('Customer login method ordering — Android standalone unaffected (#228)', () => {
    test.use({
        userAgent:
            'Mozilla/5.0 (Linux; Android 14; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36',
    });

    test('standalone + Android still leads with the link form', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await setIosStandalone(page);
        await setEnglishLanguage(page);
        await page.goto('/login');
        await page.waitForSelector('h1.page-title');

        await expect(page.locator('[data-testid="login-method-link"]')).toHaveAttribute(
            'aria-selected',
            'true',
        );
        await expect(page.locator('[data-testid="login-link-form"]')).toBeVisible();

        assertCleanConsole(consoleMessages);
    });
});
