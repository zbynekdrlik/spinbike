import { test, expect } from '@playwright/test';
import {
    loginViaAPI,
    setupConsoleCheck,
    assertCleanConsole,
    bratislavaDateOffset,
} from './helpers';

// #365: when a customer's monthly pass has expired, their NEXT real entry
// auto-renews the pass at the price of their last one (35 € here, the seeded
// monthly_pass default_price) and their credit goes NEGATIVE — and the client
// /my/balance summary card must visibly HIGHLIGHT that minus so they can see
// they owe money ("má pri mne svietiť, že som v mínuse").
//
// This drives the REAL hold-to-open door button in a real browser (the same
// path door-open.spec.ts uses), so the whole flow is exercised end-to-end:
// expired pass -> door press -> server auto-renewal -> negative credit ->
// highlighted minus on the client card.

const BASE_URL = 'http://localhost:8099';

function randSuffix(): string {
    return Array.from({ length: 8 }, () =>
        String.fromCharCode(97 + Math.floor(Math.random() * 26)),
    ).join('');
}

/**
 * Seed a login-able customer with: allow_self_entry, a unique card_code, a
 * small positive credit (20 €), and an EXPIRED monthly pass (last sold at the
 * 35 € default_price). After one door press the auto-renewal debits 35 €, so
 * credit lands at -15 €.
 */
async function seedExpiredPassCustomer(
    adminToken: string,
): Promise<{ email: string; password: string; cardCode: string }> {
    const suffix = randSuffix();
    const email = `AR-${suffix}@test.local`;
    const password = `Pw-${suffix}`;

    const seedResp = await fetch(`${BASE_URL}/api/test/seed-account`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email, password, name: `AR ${suffix}`, role: 'customer' }),
    });
    if (!seedResp.ok) throw new Error(`seed-account failed: ${seedResp.status} ${await seedResp.text()}`);
    const { user_id } = await seedResp.json();

    const cardCode = `AR-${user_id}-${suffix}`;
    // Assign card_code + allow_self_entry in one admin PUT.
    const putResp = await fetch(`${BASE_URL}/api/users/${user_id}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
        body: JSON.stringify({ card_code: cardCode, allow_self_entry: true }),
    });
    if (!putResp.ok) throw new Error(`admin PUT failed: ${putResp.status} ${await putResp.text()}`);

    // Give them 20 € of credit so the 35 € renewal lands them at -15 €.
    const creditResp = await fetch(`${BASE_URL}/api/test/seed-credit`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
        body: JSON.stringify({ barcode: cardCode, credit: 20.0 }),
    });
    if (!creditResp.ok) throw new Error(`seed-credit failed: ${creditResp.status} ${await creditResp.text()}`);

    // A monthly pass sold for an EXPLICIT 35 € that expired yesterday. Using an
    // explicit amount (not seed-expired-pass, which reads the service's
    // admin-editable default_price) keeps the renewal price deterministic in
    // the cumulative CI DB: auto_renew reads THIS row's amount, so credit lands
    // at exactly 20 - 35 = -15 regardless of what any earlier spec set the
    // monthly_pass default_price to.
    const passResp = await fetch(`${BASE_URL}/api/test/seed-transactions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
        body: JSON.stringify({
            barcode: cardCode,
            entries: [
                {
                    amount: -35.0,
                    action: 'charge',
                    service_name_sk: 'Mesačná permanentka',
                    valid_until: bratislavaDateOffset(-1),
                },
            ],
        }),
    });
    if (!passResp.ok) throw new Error(`seed-transactions failed: ${passResp.status} ${await passResp.text()}`);

    return { email, password, cardCode };
}

test.describe('Expired-pass auto-renewal + visible minus (#365)', () => {
    test('door entry with an expired pass auto-renews and lights the negative-balance highlight', async ({
        page,
        baseURL,
    }) => {
        const messages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, baseURL!, 'admin@test.com', 'admin123');
        const customer = await seedExpiredPassCustomer(adminToken);

        // Switch to the customer session.
        await page.evaluate(() => { localStorage.clear(); });
        await loginViaAPI(page, baseURL!, customer.email, customer.password);

        await page.goto('/my/balance');

        // Before the entry, credit is a positive 20 € — no highlight.
        const creditCard = page.locator('[data-testid="my-balance-credit"]');
        await expect(creditCard).toBeVisible();
        await expect(creditCard).not.toHaveClass(/card-credit--negative/);

        // Hold the door button ~1.2s to open (same interaction as door-open.spec).
        const btn = page.locator('[data-testid="door-open-button"]');
        await expect(btn).toBeVisible();
        await btn.dispatchEvent('pointerdown');
        await page.waitForTimeout(1200);
        await btn.dispatchEvent('pointerup');

        // The door physically opened (server auto-renewed instead of charging a
        // single entry).
        await expect(page.locator('[data-testid="door-banner"]')).toContainText('Door open', {
            timeout: 5000,
        });

        // The summary balance is now NEGATIVE (20 - 35 = -15) and highlighted.
        await expect(creditCard).toHaveClass(/card-credit--negative/, { timeout: 5000 });
        await expect(creditCard).toContainText('-15.00');

        // The renewal is visible in the customer's own history as an
        // 'auto-obnova' pass movement (raw note rendered verbatim).
        await expect(page.locator('[data-testid="recent-visit"]').filter({ hasText: 'auto-obnova' })).toHaveCount(
            1,
            { timeout: 5000 },
        );

        assertCleanConsole(messages);
    });
});
