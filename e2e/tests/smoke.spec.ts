import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, passwordLoginForm } from './helpers';

// Read-only post-deploy smoke tests. Safe to run against production because
// they never write data — they only verify that the freshly-deployed binary
// serves the expected UI and responds to public API probes.
//
// Selected via `npx playwright test -g '@smoke'`; skipped from the default
// local test run (those tests seed and mutate the e2e database).

const BASE = process.env.SMOKE_BASE_URL || 'http://localhost:8099';

test.describe('@smoke post-deploy', () => {
    test('schedule page renders with clean console', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await page.goto(BASE);
        await page.waitForSelector('h1.page-title', { timeout: 15000 });
        // The home page is the schedule — localized but always present.
        const title = await page.textContent('h1.page-title');
        expect(title?.length ?? 0).toBeGreaterThan(0);
        assertCleanConsole(consoleMessages);
    });

    test('login page renders with clean console', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await page.goto(`${BASE}/login`);
        await page.waitForSelector('input[type="email"]', { timeout: 15000 });
        // The page now has TWO type="email" inputs: the admin/staff password
        // form and the customer login-link section below it (#109).
        // passwordLoginForm() scopes to the form that has a password input —
        // the invariant this test is actually checking — regardless of DOM
        // order between the two sections.
        const form = passwordLoginForm(page);
        await expect(form.locator('input[type="email"]')).toBeVisible();
        await expect(form.locator('input[type="password"]')).toBeVisible();
        assertCleanConsole(consoleMessages);
    });

    test('API auth login endpoint rejects bad creds without 5xx', async ({ request }) => {
        // Just proves the server is up and routing. NO real credentials used.
        const resp = await request.post(`${BASE}/api/auth/login`, {
            data: { email: 'nobody@example.invalid', password: 'x' },
            failOnStatusCode: false,
        });
        // 401 expected (unknown email). 5xx would mean the DB / server is broken.
        expect(resp.status()).toBeGreaterThanOrEqual(400);
        expect(resp.status()).toBeLessThan(500);
    });

    test('public schedule API responds with a valid date range query', async ({ request }) => {
        const today = new Date().toISOString().slice(0, 10);
        const resp = await request.get(`${BASE}/api/classes?from=${today}&to=${today}`);
        expect(resp.ok()).toBeTruthy();
        const body = await resp.json();
        expect(Array.isArray(body)).toBe(true);
    });
});
