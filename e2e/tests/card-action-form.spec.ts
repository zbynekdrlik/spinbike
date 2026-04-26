import { test, expect, Page } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

async function activateUniqueCard(
    token: string,
    initialCredit: number,
): Promise<{ barcode: string; lastName: string }> {
    const suffix = `${Date.now()}${Math.random().toString(36).slice(2, 6)}`;
    const barcode = `AF-${suffix}`;
    const lastName = `ActionForm${suffix}`;
    const resp = await fetch(`${BASE_URL}/api/cards/activate`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ barcode, initial_credit: initialCredit, first_name: 'AF', last_name: lastName }),
    });
    if (!resp.ok) throw new Error(`activate failed: ${resp.status} ${await resp.text()}`);
    return { barcode, lastName };
}

async function openCardByLastName(page: Page, lastName: string) {
    const searchInput = page.locator('input[type="search"]');
    await searchInput.waitFor();
    await searchInput.focus();
    await page.keyboard.type(lastName, { delay: 30 });
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
}

// Playwright's selectOption doesn't accept regex for `label`. Look up the
// option's value attribute by its visible text, then select by value.
async function selectMonthlyPass(page: Page) {
    const value = await page
        .locator('[data-testid="charge-service"] option')
        .filter({ hasText: 'Monthly pass' })
        .first()
        .getAttribute('value');
    if (!value) throw new Error('Monthly pass option not found');
    await page.locator('[data-testid="charge-service"]').selectOption(value);
}

test.describe('Card action form — unified Charge / Top-up / Sell pass', () => {
    test('default state: service options include Monthly pass; Charge label is not Sell pass', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        const options = await page.locator('[data-testid="charge-service"] option').allTextContents();
        expect(options.some(o => /Monthly pass/.test(o))).toBe(true);

        const label = (await page.locator('[data-testid="charge-submit"]').textContent()) ?? '';
        expect(/predat|sell/i.test(label)).toBe(false);

        await expect(page.locator('[data-testid="valid-until-row"]')).toHaveCount(0);

        assertCleanConsole(msgs);
    });

    test('selecting Monthly pass shows date row and flips Charge → Sell pass', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        await selectMonthlyPass(page);
        await expect(page.locator('[data-testid="valid-until-row"]')).toBeVisible();
        const flipped = (await page.locator('[data-testid="charge-submit"]').textContent()) ?? '';
        expect(/predat|sell/i.test(flipped)).toBe(true);

        // Switch to a non-pass service: date row hides, label restores.
        await page.locator('[data-testid="charge-service"]').selectOption({ index: 1 });
        await expect(page.locator('[data-testid="valid-until-row"]')).toHaveCount(0);
        const restored = (await page.locator('[data-testid="charge-submit"]').textContent()) ?? '';
        expect(/predat|sell/i.test(restored)).toBe(false);

        assertCleanConsole(msgs);
    });

    test('Sell pass: dropdown flow posts to /api/payments/sell-pass and pass banner appears', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        const sellPassReq = page.waitForRequest(req =>
            req.url().endsWith('/api/payments/sell-pass') && req.method() === 'POST'
        );

        await selectMonthlyPass(page);
        // Amount auto-filled from default_price (35.00). Accept it.
        await page.locator('[data-testid="charge-submit"]').click();

        const req = await sellPassReq;
        const body = JSON.parse(req.postData() ?? '{}');
        expect(typeof body.card_id).toBe('number');
        expect(body.price).toBe(35.0);
        expect(body.valid_until).toMatch(/^\d{4}-\d{2}-\d{2}$/);

        await expect(page.locator('[data-testid="pass-banner-active"]')).toBeVisible();

        assertCleanConsole(msgs);
    });

    test('Charge: non-pass service posts to /api/payments/charge with service_id', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        const chargeReq = page.waitForRequest(req =>
            req.url().endsWith('/api/payments/charge') && req.method() === 'POST'
        );

        await page.locator('[data-testid="charge-service"]').selectOption({ index: 1 });
        await page.locator('[data-testid="charge-amount"]').fill('3.50');
        await page.locator('[data-testid="charge-submit"]').click();

        const req = await chargeReq;
        const body = JSON.parse(req.postData() ?? '{}');
        expect(body.amount).toBe(3.5);
        expect(typeof body.service_id).toBe('number');

        assertCleanConsole(msgs);
    });

    test('Top up: posts to /api/cards/topup regardless of selected service', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        const topupReq = page.waitForRequest(req =>
            req.url().endsWith('/api/cards/topup') && req.method() === 'POST'
        );

        // Even if a service is selected (including Monthly pass), Top up must ignore it.
        await selectMonthlyPass(page);
        await page.locator('[data-testid="charge-amount"]').fill('20');
        await page.locator('[data-testid="topup-submit"]').click();

        const req = await topupReq;
        const body = JSON.parse(req.postData() ?? '{}');
        expect(body.amount).toBe(20.0);
        expect(body).not.toHaveProperty('service_id');

        assertCleanConsole(msgs);
    });
});
