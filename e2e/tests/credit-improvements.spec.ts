import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

/**
 * Seed a fresh card with an active monthly pass via the test-fixtures endpoint.
 * Returns the barcode. Each test uses its own barcode so tests are independent
 * of execution order and of each other's side effects.
 */
async function seedCardWithPass(
    request: import('@playwright/test').APIRequestContext,
    token: string,
    barcode: string,
    validUntil: string,
): Promise<void> {
    const resp = await request.post(`${BASE_URL}/api/test/seed-expired-pass`, {
        data: { barcode, valid_until: validUntil },
        headers: { Authorization: `Bearer ${token}` },
    });
    expect(resp.ok()).toBeTruthy();
}

async function openCard(page: import('@playwright/test').Page, barcode: string) {
    await page.goto('/staff');
    const searchInput = page.locator('input[type="search"]');
    await searchInput.waitFor();
    await searchInput.focus();
    await page.keyboard.type(barcode, { delay: 30 });
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
}

test.describe('credit improvements', () => {
    test('charge form stays visible when a monthly pass is active', async ({ page, request }) => {
        const consoleMessages = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');

        // Future date → active pass. 2030 guarantees "active" regardless of test date.
        const barcode = 'CREDIT-IMPR-CHARGE';
        await seedCardWithPass(request, token, barcode, '2030-12-31');

        await openCard(page, barcode);

        // Active pass banner is visible.
        await expect(page.locator('[data-testid="pass-banner-active"]')).toBeVisible();

        // Log-visit buttons (pass mode) AND the charge form coexist — this is the Task 7 fix.
        await expect(page.locator('[data-testid="log-visit-btn"]').first()).toBeVisible();
        await expect(page.locator('[data-testid="charge-service"]')).toBeVisible();
        await expect(page.locator('[data-testid="charge-submit"]')).toBeVisible();

        // Actually charge for a non-pass service. global-setup creates "Spinning" as the
        // only service besides "Monthly pass"; the select filters out "Monthly pass"
        // so the first real option (index 1, skipping the "select…" placeholder) is Spinning.
        await page.locator('[data-testid="charge-service"]').selectOption({ index: 1 });
        const amountInput = page.locator('form.inline-form input[type="number"]').first();
        await amountInput.fill('2');
        await page.locator('[data-testid="charge-submit"]').click();

        // After charge, a new "charge" row should appear in the history tab. Tab defaults
        // to history; wait for the transactions table to include a row for Spinning at 2.00.
        const historyTable = page.locator('table.data-table');
        await expect(historyTable).toContainText('Spinning', { timeout: 5000 });
        await expect(historyTable).toContainText('-2.00');

        assertCleanConsole(consoleMessages);
    });

    test('staff edits monthly pass end date inline', async ({ page, request }) => {
        const consoleMessages = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');

        const barcode = 'CREDIT-IMPR-EDIT-DATE';
        await seedCardWithPass(request, token, barcode, '2030-06-15');

        await openCard(page, barcode);

        const banner = page.locator('[data-testid="pass-banner-active"]');
        await expect(banner).toBeVisible();
        // Banner shows the original date in dd.MM.yyyy format.
        await expect(banner).toContainText('15.06.2030');

        // Click pencil to enter edit mode, change date, save.
        await page.locator('[data-testid="pass-date-edit"]').click();
        const input = page.locator('[data-testid="pass-date-input"]');
        await expect(input).toBeVisible();
        await input.fill('2027-01-15');
        await page.locator('[data-testid="pass-date-save"]').click();

        // Banner refreshes: new date in dd.MM.yyyy and edit UI closes.
        await expect(banner).toContainText('15.01.2027', { timeout: 5000 });
        await expect(input).toBeHidden();

        assertCleanConsole(consoleMessages);
    });

    test('staff voids a transaction row; row greys out with voided class', async ({ page, request }) => {
        const consoleMessages = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');

        const barcode = 'CREDIT-IMPR-VOID';
        // Seeding creates a pass transaction we can void.
        await seedCardWithPass(request, token, barcode, '2030-03-01');

        await openCard(page, barcode);

        // Default tab is history; confirm the seeded transaction row is present and NOT voided.
        await page.locator('[data-testid="tab-history"]').click();
        const voidBtn = page.locator('[data-testid="txn-void"]').first();
        await expect(voidBtn).toBeVisible();

        // Accept the confirm() dialog that fires on void.
        page.once('dialog', (d) => d.accept());
        await voidBtn.click();

        // Voided row gets the .txn-row--voided class; the void button disappears for that row.
        await expect(page.locator('tr.txn-row--voided').first()).toBeVisible({ timeout: 5000 });
        await expect(page.locator('[data-testid="txn-void"]')).toHaveCount(0);

        assertCleanConsole(consoleMessages);
    });

    test('card detail has three working tabs', async ({ page, request }) => {
        const consoleMessages = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');

        const barcode = 'CREDIT-IMPR-TABS';
        await seedCardWithPass(request, token, barcode, '2030-09-20');

        await openCard(page, barcode);

        // All three tabs are visible.
        await expect(page.locator('[data-testid="tab-history"]')).toBeVisible();
        await expect(page.locator('[data-testid="tab-upcoming"]')).toBeVisible();
        await expect(page.locator('[data-testid="tab-persistent"]')).toBeVisible();

        // Default = history → the transactions table is visible.
        await expect(page.locator('table.data-table')).toBeVisible();

        // Upcoming tab → the upcoming-classes panel renders.
        await page.locator('[data-testid="tab-upcoming"]').click();
        await expect(page.locator('[data-testid="upcoming-classes"]')).toBeVisible();
        await expect(page.locator('table.data-table')).toHaveCount(0);

        // Persistent tab → the persistent-toggles panel renders.
        await page.locator('[data-testid="tab-persistent"]').click();
        await expect(page.locator('[data-testid="persistent-toggles"]')).toBeVisible();
        await expect(page.locator('[data-testid="upcoming-classes"]')).toHaveCount(0);

        // Back to history.
        await page.locator('[data-testid="tab-history"]').click();
        await expect(page.locator('table.data-table')).toBeVisible();

        assertCleanConsole(consoleMessages);
    });
});
