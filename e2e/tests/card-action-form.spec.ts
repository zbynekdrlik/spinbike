import { test, expect, Page } from '@playwright/test';
import {
    setupConsoleCheck,
    assertCleanConsole,
    loginViaAPI,
    selectMonthlyPass,
    activateUniqueCard,
} from './helpers';

const BASE_URL = 'http://localhost:8099';

async function openCardByLastName(page: Page, lastName: string) {
    const searchInput = page.locator('input[type="search"]');
    await searchInput.waitFor();
    await searchInput.focus();
    await page.keyboard.type(lastName, { delay: 30 });
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
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
        // Staff types the price every time (#17 — no auto-fill). Use 35.00
        // to match the assertion `expect(body.price).toBe(35.0)` below.
        await page.locator('[data-testid="charge-amount"]').fill('35.00');
        await page.locator('[data-testid="charge-submit"]').click();

        const req = await sellPassReq;
        const body = JSON.parse(req.postData() ?? '{}');
        expect(typeof body.user_id).toBe('number');
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

    test('Charge with empty amount surfaces inline error (no silent no-op)', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        // Pick a non-pass service. Post-#17 the input is already empty after
        // selectOption — the Ctrl+A; Delete clear below is redundant but
        // harmless and kept defensively in case a future regression
        // reintroduces auto-fill.
        await page.locator('[data-testid="charge-service"]').selectOption({ index: 1 });
        const amountInput = page.locator('[data-testid="charge-amount"]');
        await amountInput.focus();
        await amountInput.press('ControlOrMeta+a');
        await amountInput.press('Delete');
        await expect(amountInput).toHaveValue('');

        await page.locator('[data-testid="charge-submit"]').click();

        // Inline error appears; credit unchanged.
        await expect(page.locator('[data-testid="action-panel"] .alert-error')).toBeVisible();
        await expect(page.locator('[data-testid="card-credit"]')).toContainText('50.00');

        assertCleanConsole(msgs);
    });

    test('Charge with amount=0 surfaces inline error (no silent no-op)', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        await page.locator('[data-testid="charge-service"]').selectOption({ index: 1 });
        await page.locator('[data-testid="charge-amount"]').fill('0');
        await page.locator('[data-testid="charge-submit"]').click();

        await expect(page.locator('[data-testid="action-panel"] .alert-error')).toBeVisible();
        await expect(page.locator('[data-testid="card-credit"]')).toContainText('50.00');

        assertCleanConsole(msgs);
    });

    test('Top up: posts to /api/users/topup regardless of selected service', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        const topupReq = page.waitForRequest(req =>
            req.url().endsWith('/api/users/topup') && req.method() === 'POST'
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
