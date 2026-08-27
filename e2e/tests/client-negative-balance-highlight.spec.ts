import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole } from './helpers';

// #49 (was covered end-to-end via the #365 door-renewal path, removed in #374):
// when a customer's credit is NEGATIVE, the client /my/balance summary card must
// visibly HIGHLIGHT the minus so they can see they owe money ("má pri mne
// svietiť, že som v mínuse"). #374 removed visit-triggered auto-renewal, so this
// no longer drives a door press — it seeds a negative balance directly and
// asserts the highlight, which is the surviving customer-facing feature. (A
// customer's credit now goes negative via a normal charge or the daily
// jobs::pass_renewal auto-renewal, both of which are covered by Rust tests.)

const BASE_URL = 'http://localhost:8099';

function randSuffix(): string {
    return Array.from({ length: 8 }, () =>
        String.fromCharCode(97 + Math.floor(Math.random() * 26)),
    ).join('');
}

/**
 * Seed a login-able customer with a unique card_code and an explicit NEGATIVE
 * credit (-15 €), so /my/balance must render the highlighted minus.
 */
async function seedNegativeCustomer(
    adminToken: string,
): Promise<{ email: string; password: string }> {
    const suffix = randSuffix();
    const email = `NB-${suffix}@test.local`;
    const password = `Pw-${suffix}`;

    const seedResp = await fetch(`${BASE_URL}/api/test/seed-account`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email, password, name: `NB ${suffix}`, role: 'customer' }),
    });
    if (!seedResp.ok) throw new Error(`seed-account failed: ${seedResp.status} ${await seedResp.text()}`);
    const { user_id } = await seedResp.json();

    // A unique card_code so seed-credit's barcode lookup targets THIS account.
    const cardCode = `NB-${user_id}-${suffix}`;
    const putResp = await fetch(`${BASE_URL}/api/users/${user_id}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
        body: JSON.stringify({ card_code: cardCode }),
    });
    if (!putResp.ok) throw new Error(`admin PUT failed: ${putResp.status} ${await putResp.text()}`);

    // Set the credit directly to a known negative value.
    const creditResp = await fetch(`${BASE_URL}/api/test/seed-credit`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
        body: JSON.stringify({ barcode: cardCode, credit: -15.0 }),
    });
    if (!creditResp.ok) throw new Error(`seed-credit failed: ${creditResp.status} ${await creditResp.text()}`);

    return { email, password };
}

test.describe('Client negative-balance highlight (#49)', () => {
    test('a negative credit lights the negative-balance highlight on /my/balance', async ({
        page,
        baseURL,
    }) => {
        const messages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, baseURL!, 'admin@test.com', 'admin123');
        const customer = await seedNegativeCustomer(adminToken);

        // Switch to the customer session.
        await page.evaluate(() => {
            localStorage.clear();
        });
        await loginViaAPI(page, baseURL!, customer.email, customer.password);

        await page.goto('/my/balance');

        // The summary balance is NEGATIVE (-15) and highlighted.
        const creditCard = page.locator('[data-testid="my-balance-credit"]');
        await expect(creditCard).toBeVisible();
        await expect(creditCard).toHaveClass(/card-credit--negative/, { timeout: 5000 });
        await expect(creditCard).toContainText('-15.00');

        assertCleanConsole(messages);
    });
});
