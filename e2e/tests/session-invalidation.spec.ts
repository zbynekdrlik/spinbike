import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole } from './helpers';

const BASE_URL = 'http://localhost:8099';

// #268 — a browser holding a still-valid, unexpired JWT for a user that no
// longer exists (soft-deleted here, but the server treats missing/soft-
// deleted/blocked identically — see the Rust integration tests in
// users_routes.rs for the other two states) must be logged out cleanly, not
// left on a broken page. Before the fix, GET /api/my/balance returned 404
// and the customer stayed on /my/balance with an error banner. After the
// fix, the server returns 401 and the EXISTING generic
// `api::get_coded`/`handle_unauthorized` client mechanism (api.rs) clears
// the stored session and redirects to /login — no new client code needed,
// only the server status code changes.
test.describe('Stale session (deleted user) → clean logout (#268)', () => {
    test('a valid token for a soft-deleted user lands on the login screen with a cleared session and a clean console', async ({
        page,
        baseURL,
    }) => {
        const messages = setupConsoleCheck(page);

        // Seed a fresh throwaway customer and log them in via the real API —
        // loginViaAPI navigates to '/' and stores spinbike_token/spinbike_user
        // in localStorage, exactly like a real customer's browser.
        const suffix = Array.from({ length: 8 }, () =>
            String.fromCharCode(97 + Math.floor(Math.random() * 26)),
        ).join('');
        const email = `stale-${suffix}@test.local`;
        const password = `Pw-${suffix}`;
        const seedResp = await fetch(`${BASE_URL}/api/test/seed-account`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ email, password, name: `Stale ${suffix}`, role: 'customer' }),
        });
        if (!seedResp.ok) {
            throw new Error(`seed-account failed: ${seedResp.status} ${await seedResp.text()}`);
        }
        const { user_id } = await seedResp.json();

        await loginViaAPI(page, baseURL!, email, password);

        // Soft-delete the customer via the admin API — a separate raw fetch
        // (NOT loginViaAPI on this page, which would overwrite the customer
        // session we just stored with the admin's own session).
        const adminLoginResp = await fetch(`${BASE_URL}/api/auth/login`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ email: 'admin@test.com', password: 'admin123' }),
        });
        if (!adminLoginResp.ok) {
            throw new Error(`admin login failed: ${adminLoginResp.status} ${await adminLoginResp.text()}`);
        }
        const { token: adminToken } = await adminLoginResp.json();
        const delResp = await fetch(`${BASE_URL}/api/users/${user_id}`, {
            method: 'DELETE',
            headers: { Authorization: `Bearer ${adminToken}` },
        });
        expect(delResp.ok).toBeTruthy();

        // The browser still holds the now-stale customer session. Loading the
        // app root (a role-aware redirect sends a stored customer session to
        // /my/balance, which fires GET /api/my/balance) is the exact repro
        // from the bug report.
        await page.goto('/');
        await page.waitForURL('**/login', { timeout: 10000 });
        await expect(page.locator('h1.page-title')).toBeVisible();

        // The stored session must be cleared, not just abandoned in the UI.
        const token = await page.evaluate(() => localStorage.getItem('spinbike_token'));
        const user = await page.evaluate(() => localStorage.getItem('spinbike_user'));
        expect(token).toBeNull();
        expect(user).toBeNull();

        assertCleanConsole(messages);
    });
});
