import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole } from './helpers';

// Customer movements list on /my/balance (#144). The owner complained the
// customer "pohyby" show raw English DB tokens (charge/topup/visit) with no
// Slovak labels, unsigned amounts, and no pass-expiry — while the admin
// transactions view is already polished. These tests assert the customer row
// now renders the SAME EventKind labels the admin uses (via
// spinbike_core::reports::classify) with signed amounts, in both languages.

const BASE_URL = 'http://localhost:8099';

function randSuffix(): string {
    return Array.from({ length: 8 }, () =>
        String.fromCharCode(97 + Math.floor(Math.random() * 26)),
    ).join('');
}

/**
 * Seed a fresh customer (with password) plus four movements that exercise
 * every EventKind: a top-up, a spend, a monthly-pass sale (charge + valid_until),
 * and a gym visit (action=visit, €0). Explicit created_at keeps the row order
 * deterministic (newest first: topup, charge, pass, visit).
 */
async function seedCustomerWithMovements(
    adminToken: string,
): Promise<{ email: string; password: string }> {
    const suffix = randSuffix();
    const email = `MV-${suffix}@test.local`;
    const password = `Pw-${suffix}`;

    const seedResp = await fetch(`${BASE_URL}/api/test/seed-account`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email, password, name: `MV ${suffix}`, role: 'customer' }),
    });
    if (!seedResp.ok) throw new Error(`seed-account failed: ${seedResp.status} ${await seedResp.text()}`);
    const { user_id } = await seedResp.json();

    const cardCode = `MV-${user_id}-${suffix}`;
    const putResp = await fetch(`${BASE_URL}/api/users/${user_id}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
        body: JSON.stringify({ card_code: cardCode }),
    });
    if (!putResp.ok) throw new Error(`assign card_code failed: ${putResp.status} ${await putResp.text()}`);

    const future = new Date();
    future.setDate(future.getDate() + 30);
    const validUntil = future.toISOString().split('T')[0];

    const txResp = await fetch(`${BASE_URL}/api/test/seed-transactions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
        body: JSON.stringify({
            barcode: cardCode,
            entries: [
                { amount: 10.0, action: 'topup', service_name_sk: 'Kredit', created_at: '2024-11-24 16:00:00' },
                { amount: -5.0, action: 'charge', service_name_sk: 'Spinning', created_at: '2024-11-24 15:00:00' },
                { amount: -35.0, action: 'charge', service_name_sk: 'Mesačná permanentka', valid_until: validUntil, created_at: '2024-11-24 14:00:00' },
                { amount: 0.0, action: 'visit', service_name_sk: 'Spinning', created_at: '2024-11-24 13:00:00' },
            ],
        }),
    });
    if (!txResp.ok) throw new Error(`seed-transactions failed: ${txResp.status} ${await txResp.text()}`);

    return { email, password };
}

test.describe('Customer movements on /my/balance (#144)', () => {
    test('rows show localized EventKind labels + signed amounts, not raw DB tokens (EN)', async ({ page, baseURL }) => {
        const messages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, baseURL!, 'admin@test.com', 'admin123');
        const cust = await seedCustomerWithMovements(adminToken);

        await page.evaluate(() => { localStorage.clear(); });
        await loginViaAPI(page, baseURL!, cust.email, cust.password); // sets EN

        await page.goto('/my/balance');
        const rows = page.locator('[data-testid="recent-visit"]');
        await expect(rows.first()).toBeVisible({ timeout: 8000 });

        const allText = (await rows.allTextContents()).join('\n');

        // The action column must NOT leak the raw DB tokens the owner complained about.
        expect(allText).not.toMatch(/\btopup\b/);
        expect(allText).not.toMatch(/\bcharge\b/);
        expect(allText).not.toMatch(/\bvisit\b/);

        // It must show the SAME English EventKind labels the admin view uses.
        expect(allText).toContain('Top-up');            // topup  -> TopUp
        expect(allText).toContain('Spent from credit'); // charge -> Charge
        expect(allText).toContain('Sale of pass');      // valid_until -> PassSale
        expect(allText).toContain('Entry with pass');   // action=visit -> Visit

        // Amounts are signed (matches admin `{:+.2}`), so a top-up and a spend
        // are distinguishable — not the old unsigned "€10.00".
        expect(allText).toContain('+10.00');
        expect(allText).toContain('-5.00');
        expect(allText).toContain('-35.00');

        // The pass-sale row shows its expiry (the "until" suffix), like admin.
        expect(allText).toContain('until');

        // Each movement now names its service (#147), like the admin view.
        // "Spinning" links the charge + visit rows; the pass row links to the
        // monthly-pass service (English name "Monthly pass"). The top-up row
        // ("Kredit") has no matching service — it degrades to no service text,
        // not an error.
        expect(allText).toContain('Spinning');
        expect(allText).toContain('Monthly pass');

        assertCleanConsole(messages);
    });

    test('rows show Slovak vocabulary, no raw English tokens (SK — the owner complaint)', async ({ page, baseURL }) => {
        const messages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, baseURL!, 'admin@test.com', 'admin123');
        const cust = await seedCustomerWithMovements(adminToken);

        await page.evaluate(() => { localStorage.clear(); });
        await loginViaAPI(page, baseURL!, cust.email, cust.password);
        // Override the EN default set by loginViaAPI: the real customer is Slovak.
        await page.addInitScript(() => {
            try { localStorage.setItem('spinbike_lang', 'sk'); } catch { /* storage not ready */ }
        });

        await page.goto('/my/balance');
        const rows = page.locator('[data-testid="recent-visit"]');
        await expect(rows.first()).toBeVisible({ timeout: 8000 });

        const allText = (await rows.allTextContents()).join('\n');

        // Slovak labels render (the "nema slovencinu na pohyboch" fix).
        expect(allText).toContain('Dobitie kreditu');    // topup
        expect(allText).toContain('Predaj permanentky');  // pass
        expect(allText).toContain('Vstup s permanentkou'); // visit
        // #149: charge label is unaccented, matching the rest of the app's
        // Slovak convention (was "Výdaj z kreditu" — mixed diacritics).
        expect(allText).toContain('Vydaj z kreditu');
        expect(allText).not.toContain('Výdaj');

        // The raw English DB tokens are gone.
        expect(allText).not.toMatch(/\btopup\b/);
        expect(allText).not.toMatch(/\bcharge\b/);
        expect(allText).not.toMatch(/\bvisit\b/);

        // Each movement now names its service (#147), in Slovak here.
        expect(allText).toContain('Spinning');
        expect(allText).toContain('Mesačná permanentka');

        assertCleanConsole(messages);
    });
});
