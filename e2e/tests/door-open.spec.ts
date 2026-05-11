import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole, createUniqueUser } from './helpers';

const BASE_URL = 'http://localhost:8099';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Register a unique customer (email + password) and return credentials.
 * Uses /api/auth/register so the user has a password from the start.
 * Then sets allow_self_entry=true via admin PUT /api/users/{id}.
 */
async function createSelfEntryCustomer(
    adminToken: string,
    prefix: string = 'DE',
): Promise<{ user_id: number; email: string; password: string }> {
    // Random suffix keeps each test isolated in the shared E2E DB.
    const suffix = Array.from({ length: 8 }, () =>
        String.fromCharCode(97 + Math.floor(Math.random() * 26)),
    ).join('');
    const email = `${prefix}-${suffix}@test.local`;
    const password = `Pw-${suffix}`;

    // Register via the public endpoint so the user has a password hash.
    const regResp = await fetch(`${BASE_URL}/api/auth/register`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email, password, name: `${prefix} ${suffix}` }),
    });
    if (!regResp.ok) {
        throw new Error(`register failed: ${regResp.status} ${await regResp.text()}`);
    }
    const regData = await regResp.json();
    const user_id: number = regData.user.id;

    // Grant allow_self_entry via admin PUT (requires admin role).
    const putResp = await fetch(`${BASE_URL}/api/users/${user_id}`, {
        method: 'PUT',
        headers: {
            'Content-Type': 'application/json',
            Authorization: `Bearer ${adminToken}`,
        },
        body: JSON.stringify({ allow_self_entry: true }),
    });
    if (!putResp.ok) {
        throw new Error(`PUT allow_self_entry failed: ${putResp.status} ${await putResp.text()}`);
    }

    return { user_id, email, password };
}

/**
 * Seed an active monthly pass (valid 30 days from today) for a user
 * identified by card_code. Uses the /api/test/seed-transactions fixture.
 */
async function seedActiveMonthlyPass(
    adminToken: string,
    cardCode: string,
): Promise<void> {
    const futureDate = new Date();
    futureDate.setDate(futureDate.getDate() + 30);
    const validUntil = futureDate.toISOString().split('T')[0];

    const resp = await fetch(`${BASE_URL}/api/test/seed-transactions`, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
            Authorization: `Bearer ${adminToken}`,
        },
        body: JSON.stringify({
            barcode: cardCode,
            entries: [
                {
                    amount: -35.0,
                    action: 'charge',
                    service_name_sk: 'Mesačná permanentka',
                    valid_until: validUntil,
                },
            ],
        }),
    });
    if (!resp.ok) {
        throw new Error(`seed monthly pass failed: ${resp.status} ${await resp.text()}`);
    }
}

/**
 * Assign a card_code to an existing user (needed by seedActiveMonthlyPass
 * which looks up by barcode = card_code).
 */
async function assignCardCode(
    adminToken: string,
    userId: number,
    cardCode: string,
): Promise<void> {
    const resp = await fetch(`${BASE_URL}/api/users/${userId}`, {
        method: 'PUT',
        headers: {
            'Content-Type': 'application/json',
            Authorization: `Bearer ${adminToken}`,
        },
        body: JSON.stringify({ card_code: cardCode }),
    });
    if (!resp.ok) {
        throw new Error(`PUT card_code failed: ${resp.status} ${await resp.text()}`);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

test.describe('Door self-entry (#92)', () => {
    test('customer holds 2s and door opens — banner + recent visit row appear', async ({ page, baseURL }) => {
        const messages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, baseURL!, 'admin@test.com', 'admin123');
        const customer = await createSelfEntryCustomer(adminToken, 'DE');

        // Assign a unique card_code so the pass-seed fixture can find the user.
        const cardCode = `DE-pass-${customer.user_id}`;
        await assignCardCode(adminToken, customer.user_id, cardCode);
        await seedActiveMonthlyPass(adminToken, cardCode);

        // Switch to customer session.
        await page.evaluate(() => { localStorage.clear(); });
        await loginViaAPI(page, baseURL!, customer.email, customer.password);

        await page.goto('/my/balance');
        const btn = page.locator('[data-testid="door-open-button"]');
        await expect(btn).toBeVisible();

        // Simulate a 2-second hold: pointerdown then wait, then pointerup.
        await btn.dispatchEvent('pointerdown');
        await page.waitForTimeout(2200);
        await btn.dispatchEvent('pointerup');

        // Banner shows success.
        await expect(page.locator('[data-testid="door-banner"]')).toContainText(
            'Door open',
            { timeout: 5000 },
        );

        // Recent visits list refreshes — the new door entry should appear.
        const recent = page.locator('[data-testid="recent-visit"]');
        await expect(recent.first()).toContainText('door: 1st', { timeout: 5000 });

        assertCleanConsole(messages);
    });

    test('button is disabled with "Ask reception" label when allow_self_entry=false', async ({ page, baseURL }) => {
        const messages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, baseURL!, 'admin@test.com', 'admin123');

        // Create a customer WITHOUT allow_self_entry (default is false).
        const suffix = Array.from({ length: 8 }, () =>
            String.fromCharCode(97 + Math.floor(Math.random() * 26)),
        ).join('');
        const email = `NE-${suffix}@test.local`;
        const password = `Pw-${suffix}`;
        const regResp = await fetch(`${BASE_URL}/api/auth/register`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ email, password, name: `NE ${suffix}` }),
        });
        if (!regResp.ok) throw new Error(`register failed: ${regResp.status}`);

        await page.evaluate(() => { localStorage.clear(); });
        await loginViaAPI(page, baseURL!, email, password);
        await page.goto('/my/balance');

        // The button is always rendered (either as disabled "Ask reception" or
        // the interactive variant). When allow_self_entry=false the button must
        // be present AND carry the "Ask reception" / "reception" label.
        const btn = page.locator('[data-testid="door-open-button"]');
        await expect(btn).toBeVisible({ timeout: 5000 });
        await expect(btn).toBeDisabled();
        const btnText = (await btn.textContent()) ?? '';
        expect(btnText.toLowerCase()).toContain('reception');

        assertCleanConsole(messages);
    });

    test('hardware fail shows "unavailable" banner and does NOT write a visit row', async ({ page, baseURL }) => {
        const messages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, baseURL!, 'admin@test.com', 'admin123');
        const customer = await createSelfEntryCustomer(adminToken, 'HF');

        // Intercept POST /api/door/open and return 503 to simulate hardware failure.
        await page.route('**/api/door/open', (route) =>
            route.fulfill({
                status: 503,
                contentType: 'application/json',
                body: JSON.stringify({ status: 'rejected', reason: 'hardware_unavailable' }),
            }),
        );

        await page.evaluate(() => { localStorage.clear(); });
        await loginViaAPI(page, baseURL!, customer.email, customer.password);

        await page.goto('/my/balance');
        const btn = page.locator('[data-testid="door-open-button"]');
        await expect(btn).toBeVisible();

        await btn.dispatchEvent('pointerdown');
        await page.waitForTimeout(2200);
        await btn.dispatchEvent('pointerup');

        // Error banner shows "unavailable" or "reception" (the i18n key is
        // "door_unavailable" → "Door unavailable - ask reception").
        await expect(page.locator('[data-testid="door-banner"]')).toContainText(
            /unavailable|reception/i,
            { timeout: 5000 },
        );

        // No visit row should have been written — the recent-visits list stays
        // empty (no "door:" note visible).
        const recentItems = page.locator('[data-testid="recent-visit"]');
        // Wait briefly for any stale refresh to settle, then verify.
        await page.waitForTimeout(500);
        const count = await recentItems.count();
        for (let i = 0; i < count; i++) {
            const text = (await recentItems.nth(i).textContent()) ?? '';
            expect(text).not.toContain('door:');
        }

        // The intentional 503 from page.route surfaces in TWO console lines:
        //   1. browser: "Failed to load resource ... 503 (Service Unavailable)"
        //   2. Leptos:  "[warning] door open: HTTP 503" (logged by the
        //      my_balance page's post_door_open helper)
        // Both are expected for this test, not real regressions — filter.
        const filtered = messages.filter(
            (m) =>
                !m.includes('503 (Service Unavailable)') &&
                !m.includes('door open: HTTP 503'),
        );
        expect(filtered).toEqual([]);
    });

    test('admin user-edit form has allow_self_entry checkbox', async ({ page, baseURL }) => {
        const messages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, baseURL!, 'admin@test.com', 'admin123');

        // Create a unique user so we can navigate directly to them via card_code.
        const u = await createUniqueUser(adminToken, 0, 'EC');

        // Navigate to the staff card dashboard with the user pre-selected.
        await page.goto(`/staff?card=${u.card_code}`);
        await expect(page.locator('[data-testid="action-panel"]')).toBeVisible({ timeout: 8000 });

        // Click "Edit info" button (no testid — locate by English label).
        await page.locator('button', { hasText: 'Edit info' }).click();

        // The edit-info form (inside a Sheet) should render the checkbox.
        const checkbox = page.locator('[data-testid="user-edit-allow-self-entry"]');
        await expect(checkbox).toBeAttached({ timeout: 8000 });

        assertCleanConsole(messages);
    });

    test('customer JWT receives 403 from staff-only API', async ({ page, baseURL }) => {
        const messages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, baseURL!, 'admin@test.com', 'admin123');
        const customer = await createSelfEntryCustomer(adminToken, 'SC');

        await page.evaluate(() => { localStorage.clear(); });
        await loginViaAPI(page, baseURL!, customer.email, customer.password);

        // Server-side enforcement is the real gate: customer JWTs hitting
        // admin-only endpoints get 403. The client-side router doesn't gate
        // /staff (would require an extra round of role checks for every page),
        // but the API responses do. That's how customers are scoped.
        const token = await page.evaluate(() => localStorage.getItem('spinbike_token'));
        const resp = await fetch(`${BASE_URL}/api/admin/templates`, {
            headers: { Authorization: `Bearer ${token}` },
        });
        expect(resp.status).toBe(403);

        assertCleanConsole(messages);
    });

    test('customer JWT visiting /staff redirects to /my/balance (client-side gate)', async ({ page, baseURL }) => {
        const messages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, baseURL!, 'admin@test.com', 'admin123');
        const customer = await createSelfEntryCustomer(adminToken, 'RG');

        await page.evaluate(() => { localStorage.clear(); });
        await loginViaAPI(page, baseURL!, customer.email, customer.password);

        // Customer typing /staff directly into the URL bar should be bounced
        // client-side to /my/balance (server-side 403 alone leaves the page
        // rendered with empty data, which leaks UI shape).
        await page.goto('/staff');
        await expect(page).toHaveURL(/\/my\/balance$/, { timeout: 6000 });

        // Same for /reports and /settings.
        await page.goto('/reports');
        await expect(page).toHaveURL(/\/my\/balance$/, { timeout: 6000 });

        await page.goto('/settings');
        await expect(page).toHaveURL(/\/my\/balance$/, { timeout: 6000 });

        assertCleanConsole(messages);
    });

    test('admin More sheet contains Open door link', async ({ page, baseURL }) => {
        const messages = setupConsoleCheck(page);
        await loginViaAPI(page, baseURL!, 'admin@test.com', 'admin123');
        await page.goto('/staff');

        // Tap the More tab to reveal the sheet (phone-style bottom nav).
        await page.locator('[data-testid="nav-more"]').click();

        const link = page.locator('[data-testid="more-open-door"]');
        await expect(link).toBeVisible({ timeout: 6000 });
        await expect(link).toHaveAttribute('href', '/door');

        assertCleanConsole(messages);
    });
});
