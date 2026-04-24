import { test, expect, Page } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

// Regression: the sell-pass price input used a controlled prop:value with
// `format!("{:.2}", price)`, so every keystroke parsed → reformatted the
// field back to 2-decimal form. Users could not clear the field or type
// digit-by-digit — trailing characters were lost and the caret jumped.
// The fix switched to an uncontrolled node_ref + read on submit, with
// `type="text" inputmode="decimal"` for better mobile UX and comma
// decimal support (Slovak keyboards).

// Activates a fresh card with a unique barcode so this file's tests never
// share state with the other E2E files (which all contend for the three
// seeded cards and race on credit values under parallel execution).
async function activateUniqueCard(
    token: string,
    initialCredit: number,
): Promise<{ barcode: string; lastName: string }> {
    const suffix = `${Date.now()}${Math.random().toString(36).slice(2, 6)}`;
    const barcode = `PASS-${suffix}`;
    const lastName = `Passtest${suffix}`;
    const resp = await fetch(`${BASE_URL}/api/cards/activate`, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
            Authorization: `Bearer ${token}`,
        },
        body: JSON.stringify({
            barcode,
            initial_credit: initialCredit,
            first_name: 'SellPass',
            last_name: lastName,
        }),
    });
    if (!resp.ok) {
        throw new Error(`activate failed: ${resp.status} ${await resp.text()}`);
    }
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

test.describe('Monthly pass — price input UX', () => {
    test('typing a custom price char-by-char survives and is charged', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');

        // Fresh card, known starting credit — no parallel test can touch it.
        const { lastName } = await activateUniqueCard(token, 80.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);
        await expect(page.locator('[data-testid="card-credit"]')).toContainText('80.00');

        // Open the sell-pass modal.
        await page.locator('[data-testid="sell-pass-btn"]').click();
        const modal = page.locator('[data-testid="sheet-sell-pass"]');
        await expect(modal).toBeVisible();

        // Default price is 35.00 (seeded service).
        const priceInput = page.locator('[data-testid="sell-pass-price"]');
        await expect(priceInput).toHaveValue('35.00');

        // Clear and type "40" character-by-character with a small delay to
        // mimic a real user pressing keys. This exercises the per-keystroke
        // round-trip that the old controlled input broke.
        await priceInput.focus();
        await priceInput.press('ControlOrMeta+a');
        await priceInput.press('Delete');
        await page.keyboard.type('40', { delay: 50 });

        // The typed value must survive — not snap back to the default.
        await expect(priceInput).toHaveValue('40');

        // Confirm: credit drops by the typed amount, not by the default 35.
        await page.locator('[data-testid="sell-pass-confirm"]').click();
        await expect(modal).not.toBeVisible();
        await expect(page.locator('[data-testid="pass-banner-active"]')).toBeVisible();
        // 80.00 - 40.00 = 40.00
        await expect(page.locator('[data-testid="card-credit"]')).toContainText('40.00');

        assertCleanConsole(consoleMessages);
    });

    test('comma decimal separator is accepted', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');

        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);
        await expect(page.locator('[data-testid="card-credit"]')).toContainText('50.00');

        await page.locator('[data-testid="sell-pass-btn"]').click();
        const priceInput = page.locator('[data-testid="sell-pass-price"]');
        await priceInput.focus();
        await priceInput.press('ControlOrMeta+a');
        await priceInput.press('Delete');
        // Slovak keyboard — comma decimal separator.
        await page.keyboard.type('25,50', { delay: 50 });
        await expect(priceInput).toHaveValue('25,50');

        await page.locator('[data-testid="sell-pass-confirm"]').click();
        await expect(page.locator('[data-testid="sheet-sell-pass"]')).not.toBeVisible();
        await expect(page.locator('[data-testid="pass-banner-active"]')).toBeVisible();
        // 50.00 - 25.50 = 24.50 → comma normalized to dot on the backend.
        await expect(page.locator('[data-testid="card-credit"]')).toContainText('24.50');

        assertCleanConsole(consoleMessages);
    });
});
