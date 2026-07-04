import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole, setEnglishLanguage } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Magic-link welcome page (#109)', () => {
    test('invite link logs the customer in and lands on my/balance; reused link shows invalid + email form', async ({
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

        // Re-use the SAME (now-used) link — must show the friendly invalid
        // state + the request-login-link email form, never a crash.
        await page.goto(testLink);
        await page.waitForSelector('[data-testid="welcome-invalid"]', { timeout: 10000 });
        await expect(page.locator('[data-testid="login-link-form"]')).toBeVisible();
        await expect(page.locator('[data-testid="login-link-email"]')).toBeVisible();

        assertCleanConsole(consoleMessages);
    });

    test('missing token shows the invalid state with the email form directly', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await setEnglishLanguage(page);

        await page.goto('/welcome');
        await page.waitForSelector('[data-testid="welcome-invalid"]', { timeout: 10000 });
        await expect(page.locator('[data-testid="login-link-form"]')).toBeVisible();

        assertCleanConsole(consoleMessages);
    });
});
