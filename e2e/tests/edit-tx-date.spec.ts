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
        const svcResp = await fetch(`${BASE_URL}/api/services`, {
            headers: { Authorization: `Bearer ${token}` },
        });
        if (!svcResp.ok) throw new Error(`services GET failed: ${svcResp.status}`);
        const services = (await svcResp.json()) as Array<{
            id: number;
            kind: string;
            active: boolean;
        }>;
        const spinning = services.find((s) => s.kind === 'spinning' && s.active);
        if (!spinning) throw new Error('No active spinning service found');

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
        const input = page.locator('[data-testid="tx-date-input"]');
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
});
