import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole } from './helpers';

// #357: the customer could see WHAT a movement was but never WHO caused it —
// an entry they let themselves in for (door press) and one the owner logged
// at the desk rendered identically. These assert the two are now told apart
// in the customer's own history, which is the entire point of the ticket.

const BASE_URL = 'http://localhost:8099';

function randSuffix(): string {
    return Array.from({ length: 8 }, () =>
        String.fromCharCode(97 + Math.floor(Math.random() * 26)),
    ).join('');
}

/**
 * Seed a customer with exactly two movements, one of each origin:
 *
 *  - a DESK charge, made through the REAL `/api/payments/charge` endpoint as
 *    admin, so the server writes `staff_id` itself. Seeding it through the
 *    test fixture instead would leave `staff_id` NULL and the test would
 *    pass against a server that never sets it — the fixture does not write
 *    that column.
 *  - a DOOR press, seeded via the fixture's door-note convention, which is
 *    how it derives `is_door_press` on this test-only surface.
 */
async function seedBothOrigins(adminToken: string): Promise<{
    email: string;
    password: string;
    adminName: string;
}> {
    const suffix = randSuffix();
    const email = `ES-${suffix}@test.local`;
    const password = `Pw-${suffix}`;

    const seedResp = await fetch(`${BASE_URL}/api/test/seed-account`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email, password, name: `ES ${suffix}`, role: 'customer' }),
    });
    if (!seedResp.ok) {
        throw new Error(`seed-account failed: ${seedResp.status} ${await seedResp.text()}`);
    }
    const { user_id } = await seedResp.json();

    const cardCode = `ES-${user_id}-${suffix}`;
    const putResp = await fetch(`${BASE_URL}/api/users/${user_id}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
        body: JSON.stringify({ card_code: cardCode }),
    });
    if (!putResp.ok) {
        throw new Error(`assign card_code failed: ${putResp.status} ${await putResp.text()}`);
    }

    // Whose name the desk row must carry. Read it rather than hard-coding it,
    // so the assertion stays true if the seeded admin is ever renamed.
    const meResp = await fetch(`${BASE_URL}/api/my/balance`, {
        headers: { Authorization: `Bearer ${adminToken}` },
    });
    if (!meResp.ok) throw new Error(`admin balance failed: ${meResp.status}`);
    const adminName: string = (await meResp.json()).name;

    // The charge endpoint requires a service. Pick it by the stable `kind`
    // column, never by name_en — staff can rename a service in the admin UI
    // and a name-matched lookup would silently stop finding it (#329).
    const svcResp = await fetch(`${BASE_URL}/api/admin/services`, {
        headers: { Authorization: `Bearer ${adminToken}` },
    });
    if (!svcResp.ok) {
        throw new Error(`/api/admin/services failed: ${svcResp.status} ${await svcResp.text()}`);
    }
    const services = (await svcResp.json()) as Array<{ id: number; kind: string }>;
    const classService = services.find((s) => s.kind === 'group_class');
    if (!classService) throw new Error('no group_class service found');

    // DESK: the real endpoint, so the server sets staff_id.
    const chargeResp = await fetch(`${BASE_URL}/api/payments/charge`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
        body: JSON.stringify({ user_id, amount: 5.0, service_id: classService.id }),
    });
    if (!chargeResp.ok) {
        throw new Error(`charge failed: ${chargeResp.status} ${await chargeResp.text()}`);
    }

    // DOOR: the fixture reads the note prefix to set is_door_press.
    const txResp = await fetch(`${BASE_URL}/api/test/seed-transactions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
        body: JSON.stringify({
            barcode: cardCode,
            entries: [
                {
                    amount: 0.0,
                    action: 'visit',
                    note: 'door: 1st',
                    created_at: '2024-11-24 14:00:00',
                },
            ],
        }),
    });
    if (!txResp.ok) {
        throw new Error(`seed-transactions failed: ${txResp.status} ${await txResp.text()}`);
    }

    return { email, password, adminName };
}

test.describe('#357 customer sees where each entry came from', () => {
    test('door press says the customer opened it; a desk entry names who recorded it', async ({
        page,
        baseURL,
    }) => {
        const messages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, baseURL!, 'admin@test.com', 'admin123');
        const seeded = await seedBothOrigins(adminToken);

        await page.evaluate(() => {
            localStorage.clear();
        });
        await loginViaAPI(page, baseURL!, seeded.email, seeded.password); // sets EN

        await page.goto('/my/balance');
        const rows = page.locator('[data-testid="recent-visit"]');
        await expect(rows.first()).toBeVisible({ timeout: 8000 });
        await expect(rows).toHaveCount(2);

        const allText = (await rows.allTextContents()).join('\n');

        expect(allText).toContain('You opened the door');
        expect(allText).toContain(`Recorded by ${seeded.adminName}`);

        // The two must be on DIFFERENT rows — a single row carrying both
        // labels would mean the origin is being rendered from the wrong
        // transaction, which is exactly the confusion #357 removes.
        const doorRow = rows.filter({ hasText: 'You opened the door' });
        await expect(doorRow).toHaveCount(1);
        await expect(doorRow).not.toContainText('Recorded by');

        assertCleanConsole(messages);
    });

    test('the same distinction reads in Slovak', async ({ page, baseURL }) => {
        const messages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, baseURL!, 'admin@test.com', 'admin123');
        const seeded = await seedBothOrigins(adminToken);

        await page.evaluate(() => {
            localStorage.clear();
        });
        await loginViaAPI(page, baseURL!, seeded.email, seeded.password);
        // Override the EN default loginViaAPI sets — the real customer is Slovak.
        await page.addInitScript(() => {
            try {
                localStorage.setItem('spinbike_lang', 'sk');
            } catch {
                /* storage not ready */
            }
        });

        await page.goto('/my/balance');
        const rows = page.locator('[data-testid="recent-visit"]');
        await expect(rows.first()).toBeVisible({ timeout: 8000 });

        const allText = (await rows.allTextContents()).join('\n');
        expect(allText).toContain('Otvoril si dvere');
        expect(allText).toContain(`Zapisal ${seeded.adminName}`);

        assertCleanConsole(messages);
    });
});
