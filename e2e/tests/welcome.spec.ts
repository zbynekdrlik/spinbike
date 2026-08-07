import { test, expect, devices } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole, setEnglishLanguage } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Magic-link welcome page (#109)', () => {
    test('invite link logs the customer in and lands on my/balance; reopening the used link while logged in short-circuits home (#258)', async ({
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

        // loginViaAPI stored the ADMIN session in localStorage (used only to
        // reach the create-user + invite APIs); clear it so the invite link is
        // redeemed from a LOGGED-OUT state — exactly like a real customer
        // clicking their emailed link. Otherwise #258's session short-circuit
        // would skip redemption and send the existing admin session home.
        await page.evaluate(() => {
            localStorage.removeItem('spinbike_token');
            localStorage.removeItem('spinbike_user');
        });

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

        // #258 session short-circuit: reopening the SAME (now-used) link while
        // a session is ALREADY stored no longer re-redeems the token — the
        // welcome page short-circuits straight to the role home (customer -> /
        // -> RootRoute bounce to /my/balance). This is exactly what an
        // installed home-screen app does on every launch after the first (its
        // start_url permanently carries the spent install token). The
        // grace-window server behavior itself (#246) stays covered by server
        // unit + integration tests (login_tokens.rs / auth_routes.rs) — the
        // front-end simply never re-POSTs the token once a session exists, so
        // there is no invalid-link dead end and no wasted grace burn.
        await page.goto(testLink);
        await page.waitForURL('**/my/balance', { timeout: 10000 });
        await expect(page.locator('[data-testid="door-open-button"]')).toBeVisible({ timeout: 10000 });
        await expect(page.locator('[data-testid="welcome-invalid"]')).toHaveCount(0);
        const tokenAfterReopen = await page.evaluate(() => localStorage.getItem('spinbike_token'));
        expect(tokenAfterReopen).toBeTruthy();

        assertCleanConsole(consoleMessages);
    });

    // #258: simulated FIRST launch of the installed home-screen app. The app
    // opens its manifest `start_url` (`/welcome?t=<install token>&src=install`)
    // from a clean, logged-out context — the install token is the only
    // credential. Proves the whole install-token -> auto-login chain end to end,
    // minus the one thing no browser can simulate: the physical iOS "Add to
    // Home Screen" tap.
    test('simulated first launch: a minted install token on /welcome signs the app in (#258)', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);

        // Mint an install token for a real customer session the way the UI
        // does (authenticated POST /api/auth/install-token) — but WITHOUT
        // storing that session in the browser, so the /welcome open below
        // starts logged out, exactly like the installed app's first launch.
        const loginResp = await fetch(`${BASE_URL}/api/auth/login`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ email: 'customer@test.com', password: 'password123' }),
        });
        expect(loginResp.ok).toBeTruthy();
        const jwt = (await loginResp.json()).token as string;

        const mintResp = await fetch(`${BASE_URL}/api/auth/install-token`, {
            method: 'POST',
            headers: { Authorization: `Bearer ${jwt}` },
        });
        expect(mintResp.ok).toBeTruthy();
        const installToken = (await mintResp.json()).token as string;
        expect(installToken).toBeTruthy();

        // Clean context (no stored session) — the install token in start_url is
        // the ONLY credential.
        await setEnglishLanguage(page);
        await page.goto(`/welcome?t=${installToken}&src=install`);
        await page.waitForSelector('[data-testid="welcome-success"]', { timeout: 10000 });

        const token = await page.evaluate(() => localStorage.getItem('spinbike_token'));
        expect(token).toBeTruthy();

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

    // #261 round-5 ladder, leg 3: a DEAD token (unknown/garbage — the same
    // uniform-rejection shape a revoked or expired install token produces)
    // opened WITHOUT an existing session must land on the recovery screen —
    // never a raw error, never a bare login form. This is the "no session,
    // token present, redeem failed" branch of Decision 3.
    test('a dead token without a session lands on the recovery screen, no raw error (#261)', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page, { allow4xxFor: ['/api/auth/token-login'] });
        await setEnglishLanguage(page);

        await page.goto('/welcome?t=this-token-was-never-issued&src=install');
        await page.waitForSelector('[data-testid="welcome-invalid"]', { timeout: 10000 });

        await expect(page.locator('[data-testid="login-method-code"]')).toHaveAttribute(
            'aria-selected',
            'true',
        );
        await expect(page.locator('[data-testid="code-login-email-form"]')).toBeVisible();
        // No raw error text/dump anywhere on the page.
        await expect(page.locator('body')).not.toContainText('invalid_or_expired_link');
        await expect(page.locator('body')).not.toContainText('401');

        const token = await page.evaluate(() => localStorage.getItem('spinbike_token'));
        expect(token).toBeNull();

        assertCleanConsole(consoleMessages);
    });

    // #261 core design requirement: the install token is MULTI-USE — it must
    // sign the app in again on a SECOND simulated launch (a fresh, clean,
    // logged-out context — storage cleared exactly like the first-launch
    // test above), not just the first. This is what actually differs from
    // round 4's 24h single-use token.
    test('the SAME install token signs the app in on a second simulated launch (#261 multi-use)', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);

        const loginResp = await fetch(`${BASE_URL}/api/auth/login`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ email: 'customer@test.com', password: 'password123' }),
        });
        expect(loginResp.ok).toBeTruthy();
        const jwt = (await loginResp.json()).token as string;

        const mintResp = await fetch(`${BASE_URL}/api/auth/install-token`, {
            method: 'POST',
            headers: { Authorization: `Bearer ${jwt}` },
        });
        expect(mintResp.ok).toBeTruthy();
        const installToken = (await mintResp.json()).token as string;

        await setEnglishLanguage(page);

        // Launch #1 — clean, logged-out context.
        await page.goto(`/welcome?t=${installToken}&src=install`);
        await page.waitForSelector('[data-testid="welcome-success"]', { timeout: 10000 });
        expect(await page.evaluate(() => localStorage.getItem('spinbike_token'))).toBeTruthy();

        // Simulate the SAME icon being reopened after storage was cleared
        // (a real re-install, or the app being force-quit and storage
        // evicted) — a genuinely clean context redeeming the SAME token
        // again. Round 4's 24h single-use token would already be consumed
        // (used_at set) and reject this; round 5's install token must not.
        await page.evaluate(() => {
            localStorage.removeItem('spinbike_token');
            localStorage.removeItem('spinbike_user');
        });
        await page.goto(`/welcome?t=${installToken}&src=install`);
        await page.waitForSelector('[data-testid="welcome-success"]', { timeout: 10000 });
        expect(await page.evaluate(() => localStorage.getItem('spinbike_token'))).toBeTruthy();

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
        // Clear the admin session loginViaAPI stored (see test #1) so the invite
        // link redeems from a logged-out state and #258's short-circuit doesn't
        // send the existing session home instead of showing welcome-success.
        await page.evaluate(() => {
            localStorage.removeItem('spinbike_token');
            localStorage.removeItem('spinbike_user');
        });

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
        // Clear the admin session loginViaAPI stored (see test #1) so the invite
        // link redeems from a logged-out state and #258's short-circuit doesn't
        // send the existing session home instead of showing welcome-success.
        await page.evaluate(() => {
            localStorage.removeItem('spinbike_token');
            localStorage.removeItem('spinbike_user');
        });

        await page.goto(testLink);
        await page.waitForSelector('[data-testid="welcome-success"]', { timeout: 10000 });

        await expect(page.locator('[data-testid="welcome-ios-post-install-note"]')).toHaveCount(0);

        assertCleanConsole(consoleMessages);
    });
});
