import { test, expect } from '@playwright/test';
import {
    setupConsoleCheck,
    assertCleanConsole,
    loginViaAPI,
    createUniqueUser,
} from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Edit transaction date (#76)', () => {
    test('staff can backdate a charge by 3 days via row pencil', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        const { user_id, card_code } = await createUniqueUser(token, 0.0, 'TXD');

        // Look up the Spinning service so we can post a charge.
        // /api/admin/services requires staff/admin; loginViaAPI uses admin@test.com so this works.
        // No separate public /api/services endpoint exists.
        const svcResp = await fetch(`${BASE_URL}/api/admin/services`, {
            headers: { Authorization: `Bearer ${token}` },
        });
        if (!svcResp.ok) throw new Error(`/api/admin/services failed: ${svcResp.status} ${await svcResp.text()}`);
        const services = (await svcResp.json()) as Array<{
            id: number;
            name_en: string;
        }>;
        const spinning = services.find((s) => s.name_en === 'Spinning');
        if (!spinning) throw new Error('Spinning service not found in /api/admin/services response');

        // Create a charge so there is a row in the txn list.
        const chargeResp = await fetch(`${BASE_URL}/api/payments/charge`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                Authorization: `Bearer ${token}`,
            },
            body: JSON.stringify({
                user_id,
                amount: 1.0,
                service_id: spinning.id,
            }),
        });
        if (!chargeResp.ok) throw new Error(`charge POST failed: ${chargeResp.status}`);

        // Open the card via search.
        await page.goto('/staff');
        const search = page.locator('input[type="search"]');
        await search.waitFor();
        await search.focus();
        await page.keyboard.type(card_code, { delay: 30 });
        await page.locator('[data-testid="search-result"]').first().click();
        await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

        // The list should have one row; click the date-edit pencil.
        const list = page.locator('[data-testid="transactions-list"]');
        await expect(list).toBeVisible();
        const row = list.locator('[data-testid="transaction-row"]').first();
        await row.locator('[data-testid="txn-date-edit"]').click();

        // The sheet appears.
        const sheet = page.locator('[data-testid="sheet-edit-tx-date"]');
        await expect(sheet).toBeVisible();

        // Set the date input to today − 3 days.
        const target = new Date();
        target.setDate(target.getDate() - 3);
        const dd = String(target.getDate()).padStart(2, '0');
        const mm = String(target.getMonth() + 1).padStart(2, '0');
        const yyyy = target.getFullYear();
        // English DateInput formats as YYYY-MM-DD; Slovak as DD.MM.YYYY. setEnglishLanguage()
        // is called inside loginViaAPI, so we type the ISO form.
        const isoTarget = `${yyyy}-${mm}-${dd}`;
        // <DateInput> places the wrapper testid on a <div> and the input testid is suffixed with -input
        const input = page.locator('[data-testid="tx-date-input-input"]');
        await input.fill(isoTarget);
        await input.blur();

        await page.locator('[data-testid="tx-date-save"]').click();

        // Sheet closes and the row re-renders. Pull the row again and assert
        // its visible date column now reflects the new day.
        await expect(sheet).not.toBeVisible();
        const updatedRow = list.locator('[data-testid="transaction-row"]').first();
        // Date is rendered via i18n::fmt_datetime_str on tx.created_at, which
        // shows the full datetime. Asserting the date portion is enough.
        const ddSkEscaped = `${dd}\\.${mm}\\.${yyyy}`;
        await expect(updatedRow).toContainText(new RegExp(`${ddSkEscaped}|${isoTarget}`));

        assertCleanConsole(msgs);
    });

    test('EditTxDateSheet Cancel closes modal with clean console (#84)', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        const { card_code, user_id } = await createUniqueUser(token, 0.0, 'TXC');

        // Seed a Spinning charge so the txn list has a row.
        const svcResp = await fetch(`${BASE_URL}/api/admin/services`, {
            headers: { Authorization: `Bearer ${token}` },
        });
        if (!svcResp.ok) throw new Error(`/api/admin/services failed: ${svcResp.status}`);
        const services = (await svcResp.json()) as Array<{ id: number; name_en: string }>;
        const spinning = services.find((s) => s.name_en === 'Spinning');
        if (!spinning) throw new Error('Spinning service not found');
        const chargeResp = await fetch(`${BASE_URL}/api/payments/charge`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                Authorization: `Bearer ${token}`,
            },
            body: JSON.stringify({ user_id, amount: 1.0, service_id: spinning.id }),
        });
        if (!chargeResp.ok) throw new Error(`charge POST failed: ${chargeResp.status}`);

        // Open the card.
        await page.goto('/staff');
        const search = page.locator('input[type="search"]');
        await search.waitFor();
        await search.focus();
        await page.keyboard.type(card_code, { delay: 30 });
        await page.locator('[data-testid="search-result"]').first().click();
        await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

        // Click date-edit pencil to open the sheet.
        const list = page.locator('[data-testid="transactions-list"]');
        await expect(list).toBeVisible();
        const row = list.locator('[data-testid="transaction-row"]').first();
        await row.locator('[data-testid="txn-date-edit"]').click();
        const sheet = page.locator('[data-testid="sheet-edit-tx-date"]');
        await expect(sheet).toBeVisible();

        // Click Cancel — the bug under test. The sheet has no testid'd Cancel
        // button (only Save has tx-date-save), so filter by i18n text just
        // like redesign-sheets.spec.ts does for EditPassDateSheet.
        await sheet.locator('button').filter({ hasText: /zrusit|cancel/i }).click();
        await expect(sheet).not.toBeVisible({ timeout: 2000 });

        assertCleanConsole(msgs);
    });
});
