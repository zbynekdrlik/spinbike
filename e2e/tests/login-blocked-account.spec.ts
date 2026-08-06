import { test, expect } from '@playwright/test';
import { loginViaAPI, createUniqueUser, setupConsoleCheck, assertCleanConsole, passwordLoginForm } from './helpers';

const BASE_URL = 'http://localhost:8099';

// #276 — password login must not mint a token for a blocked customer. Before
// the fix, POST /api/auth/login returned 200 + a valid token for a blocked
// account, the client redirected to /my/balance, that endpoint 401'd
// (#268's session-invalidation gate), and the client wiped the session and
// bounced back to /login — forever, showing a false "session expired"
// message instead of telling the customer their account is blocked.
test.describe('Login page — blocked account (#276)', () => {
    test('a blocked customer entering the correct password stays on /login with the account-blocked message, not a bounce loop', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);

        // Seed a throwaway customer with a known password, then block it —
        // admin/staff-only APIs, driven via a real admin session.
        const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        const suffix = Array.from({ length: 8 }, () =>
            String.fromCharCode(97 + Math.floor(Math.random() * 26)),
        ).join('');
        const email = `blocked-${suffix}@test.local`;
        const password = `Pw-${suffix}`;
        const user = await createUniqueUser(adminToken, 0, 'BL', email);

        const setPwResp = await fetch(`${BASE_URL}/api/users/${user.user_id}`, {
            method: 'PUT',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
            body: JSON.stringify({ password }),
        });
        if (!setPwResp.ok) {
            throw new Error(`set password failed: ${setPwResp.status} ${await setPwResp.text()}`);
        }

        const blockResp = await fetch(`${BASE_URL}/api/users/block`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
            body: JSON.stringify({ user_id: user.user_id, blocked: true }),
        });
        if (!blockResp.ok) {
            throw new Error(`block failed: ${blockResp.status} ${await blockResp.text()}`);
        }

        // loginViaAPI stored the ADMIN session in localStorage — clear it so
        // the login attempt below runs as an anonymous browser (the real
        // scenario), and DON'T force English: this asserts the default
        // Slovak locale a first-time visitor sees (no spinbike_lang saved).
        await page.evaluate(() => {
            localStorage.removeItem('spinbike_token');
            localStorage.removeItem('spinbike_user');
        });

        await page.goto('/login');
        await page.waitForSelector('h1.page-title');

        const form = passwordLoginForm(page);
        await form.locator('input[type="email"]').fill(email);
        await form.locator('input[type="password"]').fill(password);
        await form.locator('button[type="submit"]').click();

        // The distinct, unaccented-Slovak account-blocked message — NOT the
        // generic wrong-password text, and NOT the session-expired redirect
        // text (which would only appear after a bounce, which must not
        // happen at all: no token is minted, so there is nothing to bounce).
        await page.waitForSelector('.alert.alert-error', { timeout: 5000 });
        const errorText = await page.textContent('.alert.alert-error');
        expect(errorText).toBe('Tvoj ucet je zablokovany, ozvi sa nam.');

        // Stays on /login — never redirected to / or /my/balance. Wait a
        // beat and re-check: before the fix, a bounce loop would have shown
        // up as a navigation away and back within this window; the fix
        // means there is nothing to bounce (no token was ever minted), so
        // the URL is simply stable.
        expect(page.url()).toContain('/login');
        await page.waitForTimeout(1000);
        expect(page.url()).toContain('/login');

        // No session was ever stored.
        const token = await page.evaluate(() => localStorage.getItem('spinbike_token'));
        expect(token).toBeNull();

        assertCleanConsole(consoleMessages);
    });
});
