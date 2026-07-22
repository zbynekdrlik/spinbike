import { test, expect, devices } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole, setEnglishLanguage } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Magic-link welcome page (#109)', () => {
    test('invite link logs the customer in and lands on my/balance; reused link within grace also succeeds (#246)', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        // Unique per-run so repeated CI runs against a persistent DB never collide.
        const suffix = Array.from({ length: 8 }, () =>
            String.fromCharCode(97 + Math.floor(Math.random() * 26)),
        ).join('');
        const email = `welcome-${suffix}@test.local`;

        const createResp = await fetch(`${BASE_URL}/api/users`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
            body: JSON.stringify({ name: `Welcome ${suffix}`, email, card_code: `WL-${suffix}` }),
        });
        if (!createResp.ok) {
            throw new Error(`create user failed: ${createResp.status} ${await createResp.text()}`);
        }
        const created = await createResp.json();
        const userId = created.id as number;

        const inviteResp = await fetch(`${BASE_URL}/api/users/${userId}/invite`, {
            method: 'POST',
            headers: { Authorization: `Bearer ${adminToken}` },
        });
        if (!inviteResp.ok) {
            throw new Error(`invite failed: ${inviteResp.status} ${await inviteResp.text()}`);
        }
        const inviteBody = await inviteResp.json();
        const testLink = inviteBody.test_link as string;
        expect(testLink).toBeTruthy();

        // English so the welcome-loading/success text (not asserted directly,
        // but keeps console/date formatting consistent with other specs).
        await setEnglishLanguage(page);

        // First visit — token is fresh: redeems it, stores the session, shows
        // the welcome CTA.
        await page.goto(testLink);
        await page.waitForSelector('[data-testid="welcome-success"]', { timeout: 10000 });

        const cta = page.locator('[data-testid="welcome-cta"]');
        await expect(cta).toBeVisible();
        await expect(cta).toHaveAttribute('href', '/my/balance');

        const token = await page.evaluate(() => localStorage.getItem('spinbike_token'));
        expect(token).toBeTruthy();

        await cta.click();
        await page.waitForURL('**/my/balance', { timeout: 10000 });
        await expect(page.locator('[data-testid="door-open-button"]')).toBeVisible({ timeout: 10000 });

        // Re-use the SAME (now-used) link within the 10-min grace window
        // (#246) — the dominant iPhone double-open (mail-app webview opens
        // it first, the real browser/installed PWA reopens it second) must
        // NOT dead-end: the second open succeeds too, same as the first.
        // Post-grace rejection is covered at the server unit-test level
        // (login_tokens.rs backdates used_at directly) — an E2E test cannot
        // wait 10 minutes.
        await page.goto(testLink);
        await page.waitForSelector('[data-testid="welcome-success"]', { timeout: 10000 });
        await expect(page.locator('[data-testid="welcome-cta"]')).toBeVisible();
        const tokenAfterReuse = await page.evaluate(() => localStorage.getItem('spinbike_token'));
        expect(tokenAfterReuse).toBeTruthy();

        assertCleanConsole(consoleMessages);
    });

    // #247: the invalid-token screen now ALWAYS leads with the code method
    // (regardless of platform) — the link the client just tried already
    // failed, so re-offering the same link method first would repeat the
    // exact failure. The link method stays reachable via the toggle.
    test('missing token shows the invalid state, leading with the code form (#247)', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await setEnglishLanguage(page);

        await page.goto('/welcome');
        await page.waitForSelector('[data-testid="welcome-invalid"]', { timeout: 10000 });

        await expect(page.locator('[data-testid="login-method-code"]')).toHaveAttribute(
            'aria-selected',
            'true',
        );
        await expect(page.locator('[data-testid="code-login-email-form"]')).toBeVisible();
        await expect(page.locator('[data-testid="login-link-form"]')).toHaveCount(0);

        // The link method stays reachable via the toggle.
        await page.click('[data-testid="login-method-link"]');
        await expect(page.locator('[data-testid="login-link-form"]')).toBeVisible();
        await expect(page.locator('[data-testid="code-login-email-form"]')).toHaveCount(0);

        assertCleanConsole(consoleMessages);
    });
});

// #228 — iOS-only post-install note under the install guide: an iOS
// home-screen web app is storage-partitioned from Safari, so the magic link
// that just logged the client in here does NOT carry over — the installed
// app will ask them to log in once more via the emailed code (#227), not a
// link. Android/Chromium shares storage between the browser and the
// installed PWA, so no such note applies there.
async function inviteAndGetWelcomeLink(page: import('@playwright/test').Page): Promise<string> {
    const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
    const suffix = Array.from({ length: 8 }, () =>
        String.fromCharCode(97 + Math.floor(Math.random() * 26)),
    ).join('');
    const email = `welcome-ios-${suffix}@test.local`;

    const createResp = await fetch(`${BASE_URL}/api/users`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
        body: JSON.stringify({ name: `Welcome iOS ${suffix}`, email, card_code: `WLI-${suffix}` }),
    });
    if (!createResp.ok) {
        throw new Error(`create user failed: ${createResp.status} ${await createResp.text()}`);
    }
    const created = await createResp.json();

    const inviteResp = await fetch(`${BASE_URL}/api/users/${created.id}/invite`, {
        method: 'POST',
        headers: { Authorization: `Bearer ${adminToken}` },
    });
    if (!inviteResp.ok) {
        throw new Error(`invite failed: ${inviteResp.status} ${await inviteResp.text()}`);
    }
    const inviteBody = await inviteResp.json();
    const testLink = inviteBody.test_link as string;
    expect(testLink).toBeTruthy();
    return testLink;
}

const iPhone = devices['iPhone 13'];

test.describe('Welcome page — iOS post-install note (#228)', () => {
    test.use({
        userAgent: iPhone.userAgent,
        viewport: iPhone.viewport,
        isMobile: iPhone.isMobile,
        hasTouch: iPhone.hasTouch,
    });

    test('iOS success state shows the post-install note', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        const testLink = await inviteAndGetWelcomeLink(page);
        await setEnglishLanguage(page);

        await page.goto(testLink);
        await page.waitForSelector('[data-testid="welcome-success"]', { timeout: 10000 });

        await expect(page.locator('[data-testid="welcome-ios-post-install-note"]')).toBeVisible();

        assertCleanConsole(consoleMessages);
    });
});

test.describe('Welcome page — Android does not show the iOS post-install note (#228)', () => {
    test.use({
        userAgent:
            'Mozilla/5.0 (Linux; Android 14; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36',
    });

    test('Android success state does not show the note', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        const testLink = await inviteAndGetWelcomeLink(page);
        await setEnglishLanguage(page);

        await page.goto(testLink);
        await page.waitForSelector('[data-testid="welcome-success"]', { timeout: 10000 });

        await expect(page.locator('[data-testid="welcome-ios-post-install-note"]')).toHaveCount(0);

        assertCleanConsole(consoleMessages);
    });
});
