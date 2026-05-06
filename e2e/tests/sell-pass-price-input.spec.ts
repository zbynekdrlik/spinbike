import { test, expect, Page } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI, selectMonthlyPass } from './helpers';

const BASE_URL = 'http://localhost:8099';

async function createUniqueUser(
    token: string,
    initialCredit: number,
): Promise<{ card_code: string; lastName: string }> {
    const suffix = `${Date.now()}${Math.random().toString(36).slice(2, 6)}`;
    const cardCode = `PASS-${suffix}`;
    const lastName = `Passtest${suffix}`;
    const resp = await fetch(`${BASE_URL}/api/users`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ name: `SellPass ${lastName}`, initial_credit: initialCredit, card_code: cardCode }),
    });
    if (!resp.ok) throw new Error(`createUniqueUser failed: ${resp.status} ${await resp.text()}`);
    return { card_code: cardCode, lastName };
}

async function openCardByLastName(page: Page, lastName: string) {
    const searchInput = page.locator('input[type="search"]');
    await searchInput.waitFor();
    await searchInput.focus();
    await page.keyboard.type(lastName, { delay: 30 });
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
}

test.describe('Sell pass — unified form price input', () => {
    test('typing a custom price char-by-char survives and is charged', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await createUniqueUser(token, 80.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        await selectMonthlyPass(page);
        // Post-#17: input starts empty; type 40 char-by-char to verify the
        // price input accepts keyboard input (was: clear auto-filled 35.00
        // then type). The Ctrl+A; Delete clear below is redundant but kept
        // defensively in case a future regression reintroduces auto-fill.
        const amountInput = page.locator('[data-testid="charge-amount"]');
        await expect(amountInput).toHaveValue('');
        await amountInput.focus();
        await amountInput.press('ControlOrMeta+a');
        await amountInput.press('Delete');
        await page.keyboard.type('40', { delay: 50 });
        await expect(amountInput).toHaveValue('40');

        await page.locator('[data-testid="charge-submit"]').click();
        await expect(page.locator('[data-testid="pass-banner-active"]')).toBeVisible();
        // 80 - 40 = 40.
        await expect(page.locator('[data-testid="card-credit"]')).toContainText('40.00');

        assertCleanConsole(msgs);
    });

    test('empty amount → inline error and no pass is sold', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await createUniqueUser(token, 80.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        await selectMonthlyPass(page);
        // Clear the auto-filled price.
        const amountInput = page.locator('[data-testid="charge-amount"]');
        await amountInput.focus();
        await amountInput.press('ControlOrMeta+a');
        await amountInput.press('Delete');
        await expect(amountInput).toHaveValue('');

        await page.locator('[data-testid="charge-submit"]').click();

        // Inline error appears in the action panel; no banner; credit unchanged.
        await expect(page.locator('[data-testid="action-panel"] .alert-error')).toBeVisible();
        await expect(page.locator('[data-testid="pass-banner-active"]')).not.toBeVisible();
        await expect(page.locator('[data-testid="card-credit"]')).toContainText('80.00');

        assertCleanConsole(msgs);
    });

    test('comma decimal separator is accepted', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await createUniqueUser(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        await selectMonthlyPass(page);
        const amountInput = page.locator('[data-testid="charge-amount"]');
        await amountInput.focus();
        await amountInput.press('ControlOrMeta+a');
        await amountInput.press('Delete');
        // Slovak keyboard — comma decimal separator.
        await page.keyboard.type('25,50', { delay: 50 });
        await expect(amountInput).toHaveValue('25,50');

        await page.locator('[data-testid="charge-submit"]').click();
        await expect(page.locator('[data-testid="pass-banner-active"]')).toBeVisible();
        // 50.00 - 25.50 = 24.50 → comma normalized to dot on the backend.
        await expect(page.locator('[data-testid="card-credit"]')).toContainText('24.50');

        assertCleanConsole(msgs);
    });
});
