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
        // (mirrors schedule.rs's own current_week_dates(), which computes
        // from the BROWSER's local time via js_sys::Date). Format from the
        // same LOCAL date components throughout — never round-trip through
        // toISOString() (UTC), which disagrees with local time overnight
        // (see helpers.ts's own #251 warning against exactly this).
        const fmt = (d: Date) =>
            `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
        const now = new Date();
        const dow = now.getDay(); // 0=Sun..6=Sat
        const daysSinceMonday = dow === 0 ? 6 : dow - 1;
        const monday = new Date(now);
        monday.setDate(now.getDate() - daysSinceMonday);
        const weekDates: string[] = [];
        for (let i = 0; i < 7; i++) {
            const d = new Date(monday);
            d.setDate(monday.getDate() + i);
            weekDates.push(fmt(d));
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
        expect(slotAfter).toBeDefined();
        expect(slotAfter?.booked).toBe(slot.booked);

        assertCleanConsole(messages);
    });
});

// #275 — follow-up to #268: the redirect itself already worked (a stale
// session correctly bounces to /login and clears storage), but the customer
// used to land on a BARE login form with no explanation. This carries the
// reason across the hard `set_href` navigation via a one-shot sessionStorage
// flag (api.rs's handle_unauthorized -> auth.rs's mark_session_expired) and
// renders it once on LoginPage (auth.rs's take_session_expired_flag, which
// clears the flag as it reads it).
test.describe('Session-invalidation redirect shows an explanation (#275)', () => {
    test('a stale-session redirect shows the notice once, then clears it (a reload does not re-show it)', async ({
        page,
        baseURL,
    }) => {
        const messages = setupConsoleCheck(page);

        const suffix = Array.from({ length: 8 }, () =>
            String.fromCharCode(97 + Math.floor(Math.random() * 26)),
        ).join('');
        const email = `expnotice-${suffix}@test.local`;
        const password = `Pw-${suffix}`;
        const seedResp = await fetch(`${BASE_URL}/api/test/seed-account`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                email,
                password,
                name: `Expnotice ${suffix}`,
                role: 'customer',
            }),
        });
        if (!seedResp.ok) {
            throw new Error(`seed-account failed: ${seedResp.status} ${await seedResp.text()}`);
        }
        const { user_id } = await seedResp.json();

        await loginViaAPI(page, baseURL!, email, password);

        // Block via a raw admin fetch — NOT loginViaAPI on this page, which
        // would overwrite the customer session we just stored.
        const adminLoginResp = await fetch(`${BASE_URL}/api/auth/login`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ email: 'admin@test.com', password: 'admin123' }),
        });
        if (!adminLoginResp.ok) {
            throw new Error(
                `admin login failed: ${adminLoginResp.status} ${await adminLoginResp.text()}`,
            );
        }
        const { token: adminToken } = await adminLoginResp.json();
        const blockResp = await fetch(`${BASE_URL}/api/users/block`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
            body: JSON.stringify({ user_id, blocked: true }),
        });
        expect(blockResp.ok).toBeTruthy();

        // The browser still holds the now-blocked customer's session.
        // Loading /my/balance fires GET /api/my/balance, gets 401, and the
        // client's handle_unauthorized redirects to /login.
        await page.goto('/');
        await page.waitForURL('**/login', { timeout: 10000 });

        const notice = page.locator('[data-testid="session-expired-notice"]');
        await expect(notice).toBeVisible();
        // English, not Slovak: loginViaAPI's setEnglishLanguage registers a
        // page.addInitScript that re-fires on EVERY later navigation for the
        // lifetime of this page (including the /login redirect below) — a
        // documented e2e-testing skill gotcha (#276), not a locale bug.
        await expect(notice).toContainText('Your session is no longer valid');

        const token = await page.evaluate(() => localStorage.getItem('spinbike_token'));
        const user = await page.evaluate(() => localStorage.getItem('spinbike_user'));
        expect(token).toBeNull();
        expect(user).toBeNull();

        // A manual reload must NOT re-show the notice — the flag is
        // read-once-then-cleared, not tied to the session state.
        await page.reload();
        await expect(page.locator('[data-testid="session-expired-notice"]')).not.toBeVisible();

        assertCleanConsole(messages);
    });

    test('a wrong-password login shows only the credentials error, never the session-expired notice', async ({
        page,
    }) => {
        const messages = setupConsoleCheck(page);

        await page.goto('/login');
        // Fresh page load, no prior stale-session redirect in this test —
        // confirms the notice is NOT some default-shown element.
        await expect(
            page.locator('[data-testid="session-expired-notice"]'),
        ).not.toBeVisible();

        await page.fill('input[type="email"]', 'nonexistent-wrongpw@test.local');
        await page.fill('input[type="password"]', 'definitely-wrong-password');
        await page.click('button[type="submit"]');

        await page.waitForSelector('.alert.alert-error', { timeout: 5000 });
        const errorText = await page.textContent('.alert.alert-error');
        expect(errorText).toContain('Nespravny email alebo heslo');

        await expect(
            page.locator('[data-testid="session-expired-notice"]'),
        ).not.toBeVisible();

        assertCleanConsole(messages);
    });

    // Review follow-up (#275): door_button.rs sends its OWN raw request and
    // has a SECOND, independent inline 401 handler (never routes through
    // api.rs's handle_unauthorized) — a session invalidated mid-press must
    // show the same notice, not the original bare-login-form bug via this
    // second code path.
    test('a session invalidated while holding the door button also shows the notice', async ({
        page,
        baseURL,
    }) => {
        const messages = setupConsoleCheck(page);

        const adminLoginResp = await fetch(`${BASE_URL}/api/auth/login`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ email: 'admin@test.com', password: 'admin123' }),
        });
        if (!adminLoginResp.ok) {
            throw new Error(
                `admin login failed: ${adminLoginResp.status} ${await adminLoginResp.text()}`,
            );
        }
        const { token: adminToken } = await adminLoginResp.json();

        const suffix = Array.from({ length: 8 }, () =>
            String.fromCharCode(97 + Math.floor(Math.random() * 26)),
        ).join('');
        const email = `doorexp-${suffix}@test.local`;
        const password = `Pw-${suffix}`;
        const seedResp = await fetch(`${BASE_URL}/api/test/seed-account`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                email,
                password,
                name: `Doorexp ${suffix}`,
                role: 'customer',
            }),
        });
        if (!seedResp.ok) {
            throw new Error(`seed-account failed: ${seedResp.status} ${await seedResp.text()}`);
        }
        const { user_id } = await seedResp.json();

        const allowResp = await fetch(`${BASE_URL}/api/users/${user_id}`, {
            method: 'PUT',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
            body: JSON.stringify({ allow_self_entry: true }),
        });
        if (!allowResp.ok) {
            throw new Error(
                `PUT allow_self_entry failed: ${allowResp.status} ${await allowResp.text()}`,
            );
        }

        await loginViaAPI(page, baseURL!, email, password);

        // Block AFTER logging in — the browser still holds the now-dead
        // session, same shape as the other tests in this file.
        const blockResp = await fetch(`${BASE_URL}/api/users/block`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
            body: JSON.stringify({ user_id, blocked: true }),
        });
        expect(blockResp.ok).toBeTruthy();

        await page.goto('/my/balance');
        const btn = page.locator('[data-testid="door-open-button"]');
        await expect(btn).toBeVisible();

        // Hold 1s+ so the press actually fires (the hold-guard, #266) —
        // the in-flight POST /api/door/open gets 401 for the now-blocked
        // caller, hit via door_button.rs's OWN inline handler.
        await btn.dispatchEvent('pointerdown');
        await page.waitForTimeout(1200);
        await btn.dispatchEvent('pointerup');

        await page.waitForURL('**/login', { timeout: 10000 });
        const notice = page.locator('[data-testid="session-expired-notice"]');
        await expect(notice).toBeVisible();
        // English: loginViaAPI's setEnglishLanguage init script (see the
        // first test in this describe block above).
        await expect(notice).toContainText('Your session is no longer valid');

        const token = await page.evaluate(() => localStorage.getItem('spinbike_token'));
        const user = await page.evaluate(() => localStorage.getItem('spinbike_user'));
        expect(token).toBeNull();
        expect(user).toBeNull();

        assertCleanConsole(messages);
    });
});
