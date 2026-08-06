import { test, expect, Page } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole } from './helpers';

// Customer-facing Slovak wording for the monthly-pass status on
// /my/balance (#263). "preplatok" means overpayment/refund — it is NOT
// the correct Slovak word for a time-based pass. The CEO-approved fix
// uses "permanentka" (the same term already used elsewhere in the app,
// e.g. edit_pass_date's "Zmenit koniec permanentky"). These tests lock
// the corrected wording in BOTH states the card-pass block can render:
// no active pass, and an active pass with an expiry date.

const BASE_URL = 'http://localhost:8099';

function randSuffix(): string {
    return Array.from({ length: 8 }, () =>
        String.fromCharCode(97 + Math.floor(Math.random() * 26)),
    ).join('');
}

async function seedCustomer(prefix: string): Promise<{ user_id: number; email: string; password: string }> {
    const suffix = randSuffix();
    const email = `${prefix}-${suffix}@test.local`;
    const password = `Pw-${suffix}`;
    const resp = await fetch(`${BASE_URL}/api/test/seed-account`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email, password, name: `${prefix} ${suffix}`, role: 'customer' }),
    });
    if (!resp.ok) throw new Error(`seed-account failed: ${resp.status} ${await resp.text()}`);
    const { user_id } = await resp.json();
    return { user_id, email, password };
}

async function assignCardCode(adminToken: string, userId: number, cardCode: string): Promise<void> {
    const resp = await fetch(`${BASE_URL}/api/users/${userId}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
        body: JSON.stringify({ card_code: cardCode }),
    });
    if (!resp.ok) throw new Error(`PUT card_code failed: ${resp.status} ${await resp.text()}`);
}

async function seedActiveMonthlyPass(adminToken: string, cardCode: string): Promise<void> {
    const futureDate = new Date();
    futureDate.setDate(futureDate.getDate() + 30);
    const validUntil = futureDate.toISOString().split('T')[0];

    const resp = await fetch(`${BASE_URL}/api/test/seed-transactions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
        body: JSON.stringify({
            barcode: cardCode,
            entries: [
                { amount: -35.0, action: 'charge', service_name_sk: 'Mesačná permanentka', valid_until: validUntil },
            ],
        }),
    });
    if (!resp.ok) throw new Error(`seed monthly pass failed: ${resp.status} ${await resp.text()}`);
}

/** Switch the current session to `email`/`password` with the SK locale
 * (loginViaAPI defaults to EN — see the e2e-testing skill's gotcha).
 *
 * `page.goto('/')` runs FIRST — on a fresh context (no prior navigation,
 * e.g. no earlier admin loginViaAPI call in this test) the page is still
 * `about:blank`, and `localStorage` access there throws a SecurityError
 * (opaque origin). Navigating establishes a real origin before we touch
 * storage; loginViaAPI's own internal `goto('/')` afterward is a harmless
 * no-op re-navigation. */
async function loginAsCustomerSk(page: Page, baseURL: string, email: string, password: string): Promise<void> {
    await page.goto('/');
    await page.evaluate(() => {
        localStorage.clear();
    });
    await loginViaAPI(page, baseURL, email, password);
    await page.addInitScript(() => {
        try {
            localStorage.setItem('spinbike_lang', 'sk');
        } catch {
            /* storage not ready */
        }
    });
}

test.describe('/my/balance monthly-pass wording is "permanentka", never "preplatok" (#263)', () => {
    test('no active pass — shows "permanentku" wording, not "preplatok"', async ({ page, baseURL }) => {
        const messages = setupConsoleCheck(page);
        const customer = await seedCustomer('PW');

        await loginAsCustomerSk(page, baseURL!, customer.email, customer.password);
        await page.goto('/my/balance');

        const passBlock = page.locator('[data-testid="my-balance-pass"]');
        await expect(passBlock).toBeVisible();
        await expect(passBlock).toContainText('permanentku');
        await expect(passBlock).not.toContainText('preplatok');

        assertCleanConsole(messages);
    });

    test('active pass — shows "permanentka platna do" wording, not "preplatok"', async ({ page, baseURL }) => {
        const messages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, baseURL!, 'admin@test.com', 'admin123');
        const customer = await seedCustomer('PA');

        const cardCode = `PA-pass-${customer.user_id}`;
        await assignCardCode(adminToken, customer.user_id, cardCode);
        await seedActiveMonthlyPass(adminToken, cardCode);

        await loginAsCustomerSk(page, baseURL!, customer.email, customer.password);
        await page.goto('/my/balance');

        const passBlock = page.locator('[data-testid="my-balance-pass"]');
        await expect(passBlock).toBeVisible();
        await expect(passBlock).toContainText('permanentka platna do');
        await expect(passBlock).not.toContainText('preplatok');

        assertCleanConsole(messages);
    });
});
