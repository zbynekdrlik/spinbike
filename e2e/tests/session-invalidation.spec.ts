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

// #277 — a blocked customer's booking attempt must log them out cleanly
// instead of succeeding (before the fix: create_booking had no session check
// at all, so a blocked customer with a still-valid JWT could keep booking
// classes indefinitely). Same client mechanism as #268 above — the server
// now returns 401 session_invalid, and the EXISTING generic
// `api::post`/`handle_unauthorized` client code (api.rs) clears the stored
// session and redirects to /login; no new frontend code needed.
test.describe('Blocked user booking attempt → clean logout (#277)', () => {
    test('clicking Book with a blocked session redirects to login with a cleared session and a clean console', async ({
        page,
        baseURL,
    }) => {
        const messages = setupConsoleCheck(page);

        const suffix = Array.from({ length: 8 }, () =>
            String.fromCharCode(97 + Math.floor(Math.random() * 26)),
        ).join('');
        const email = `blk-${suffix}@test.local`;
        const password = `Pw-${suffix}`;
        const seedResp = await fetch(`${BASE_URL}/api/test/seed-account`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ email, password, name: `Blk ${suffix}`, role: 'customer' }),
        });
        if (!seedResp.ok) {
            throw new Error(`seed-account failed: ${seedResp.status} ${await seedResp.text()}`);
        }
        const { user_id } = await seedResp.json();

        // Find a real, currently-bookable occurrence in THIS week — the
        // SchedulePage's day-picker only shows the current Mon-Sun week
        // (mirrors schedule.rs's own current_week_dates()).
        const now = new Date();
        const dow = now.getDay(); // 0=Sun..6=Sat
        const daysSinceMonday = dow === 0 ? 6 : dow - 1;
        const monday = new Date(now);
        monday.setDate(now.getDate() - daysSinceMonday);
        const weekDates: string[] = [];
        for (let i = 0; i < 7; i++) {
            const d = new Date(monday);
            d.setDate(monday.getDate() + i);
            weekDates.push(d.toISOString().split('T')[0]);
        }
        const occResp = await fetch(
            `${BASE_URL}/api/classes?from=${weekDates[0]}&to=${weekDates[6]}`,
        );
        if (!occResp.ok) {
            throw new Error(`list classes failed: ${occResp.status} ${await occResp.text()}`);
        }
        const occurrences: Array<{
            template_id: number;
            date: string;
            cancelled: boolean;
            booked: number;
            capacity: number;
        }> = await occResp.json();
        const slot = occurrences.find((o) => !o.cancelled && o.booked < o.capacity);
        if (!slot) {
            throw new Error(`no bookable occurrence this week: ${JSON.stringify(occurrences)}`);
        }
        const dayIdx = weekDates.indexOf(slot.date);

        await loginViaAPI(page, baseURL!, email, password);

        // Block the customer via the admin API — a separate raw fetch (NOT
        // loginViaAPI on this page, which would overwrite the customer
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
        const blockResp = await fetch(`${BASE_URL}/api/users/block`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
            body: JSON.stringify({ user_id, blocked: true }),
        });
        expect(blockResp.ok).toBeTruthy();

        // The browser still holds the now-blocked customer's session.
        // Navigate to the schedule and attempt to book the found slot.
        await page.goto('/schedule');
        await page.locator('.day-btn').nth(dayIdx).click();
        const bookBtn = page.locator(
            `[data-testid="book-${slot.template_id}-${slot.date}"]`,
        );
        await expect(bookBtn).toBeVisible({ timeout: 5000 });
        await bookBtn.click();

        await page.waitForURL('**/login', { timeout: 10000 });
        await expect(page.locator('h1.page-title')).toBeVisible();

        const token = await page.evaluate(() => localStorage.getItem('spinbike_token'));
        const user = await page.evaluate(() => localStorage.getItem('spinbike_user'));
        expect(token).toBeNull();
        expect(user).toBeNull();

        // No booking must have been created for the blocked user — checked
        // via the admin token minted above, reading the occurrence back.
        const occAfterResp = await fetch(
            `${BASE_URL}/api/classes?from=${slot.date}&to=${slot.date}`,
            { headers: { Authorization: `Bearer ${adminToken}` } },
        );
        const occAfter: Array<{ template_id: number; booked: number }> =
            await occAfterResp.json();
        const slotAfter = occAfter.find((o) => o.template_id === slot.template_id);
        expect(slotAfter?.booked ?? 0).toBe(slot.booked);

        assertCleanConsole(messages);
    });
});
