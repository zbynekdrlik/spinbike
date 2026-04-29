import { test, expect, Page } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

async function activateUniqueCard(
    token: string,
    initialCredit: number,
): Promise<{ barcode: string; lastName: string }> {
    const suffix = `${Date.now()}${Math.random().toString(36).slice(2, 6)}`;
    const barcode = `NOTE-${suffix}`;
    const lastName = `Note${suffix}`;
    const resp = await fetch(`${BASE_URL}/api/cards/activate`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ barcode, initial_credit: initialCredit, first_name: 'NT', last_name: lastName }),
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

async function chargeWithNote(page: Page, amount: string, note: string) {
    const refreshOption = page
        .locator('[data-testid="charge-service"] option')
        .filter({ hasText: /Refreshments|Občerstvenie/ })
        .first();
    await expect(refreshOption).toBeAttached();
    const value = await refreshOption.getAttribute('value');
    if (!value) throw new Error('Refreshments option had no value');
    await page.locator('[data-testid="charge-service"]').selectOption(value);
    await page.locator('[data-testid="charge-amount"]').fill(amount);
    if (note.length > 0) {
        await page.locator('[data-testid="txn-note-input"]').fill(note);
    }
    const chargeResp = page.waitForResponse(
        (r) => r.url().includes('/api/payments/charge') && r.request().method() === 'POST',
    );
    await page.locator('[data-testid="charge-submit"]').click();
    const resp = await chargeResp;
    expect(resp.ok()).toBe(true);
}

test.describe('Transaction notes — issue #26', () => {

    test('charge with note shows note inline on card history', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);
        await chargeWithNote(page, '2.50', 'Proteinová tyčinka');

        const noteRow = page.locator('[data-testid="txn-note-text"]').first();
        await expect(noteRow).toBeVisible();
        await expect(noteRow).toContainText('Proteinová tyčinka');

        assertCleanConsole(msgs);
    });

    test('note appears on report activity feed', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);
        const noteText = `feed-${Date.now()}`;
        await chargeWithNote(page, '1.00', noteText);

        const today = new Date().toISOString().slice(0, 10);
        await page.goto(`/reports?date=${today}`);
        const feedNote = page
            .locator('[data-testid="feed-row"]')
            .filter({ has: page.locator('[data-testid="feed-note"]', { hasText: noteText }) })
            .first();
        await expect(feedNote).toBeVisible();

        assertCleanConsole(msgs);
    });

    test('inline pencil edits an existing note', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);
        await chargeWithNote(page, '1.50', 'old note');

        // Edit the note on the most recent row.
        const firstRow = page.locator('[data-testid="transactions-list"] .list-row').first();
        await firstRow.locator('[data-testid="txn-note-edit"]').click();
        const editInput = firstRow.locator('[data-testid="txn-note-edit-input"]');
        await expect(editInput).toBeVisible();
        await editInput.fill('new note');
        const patchResp = page.waitForResponse(
            (r) => r.url().match(/\/api\/transactions\/\d+\/note/) !== null && r.request().method() === 'PATCH',
        );
        await firstRow.locator('[data-testid="txn-note-save"]').click();
        const resp = await patchResp;
        expect(resp.ok()).toBe(true);

        await expect(firstRow.locator('[data-testid="txn-note-text"]')).toContainText('new note');
        assertCleanConsole(msgs);
    });

    test('clearing a note removes the note line', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);
        await chargeWithNote(page, '1.50', 'temporary');

        const firstRow = page.locator('[data-testid="transactions-list"] .list-row').first();
        await firstRow.locator('[data-testid="txn-note-edit"]').click();
        await firstRow.locator('[data-testid="txn-note-edit-input"]').fill('');
        const patchResp = page.waitForResponse(
            (r) => r.url().match(/\/api\/transactions\/\d+\/note/) !== null && r.request().method() === 'PATCH',
        );
        await firstRow.locator('[data-testid="txn-note-save"]').click();
        await patchResp;

        await expect(firstRow.locator('[data-testid="txn-note-text"]')).toHaveCount(0);
        assertCleanConsole(msgs);
    });

    test('charge without a note renders no note line', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);
        await chargeWithNote(page, '1.50', '');

        const firstRow = page.locator('[data-testid="transactions-list"] .list-row').first();
        await expect(firstRow.locator('[data-testid="txn-note-text"]')).toHaveCount(0);
        // Pencil is still visible (lets staff add a note later).
        await expect(firstRow.locator('[data-testid="txn-note-edit"]')).toBeVisible();
        assertCleanConsole(msgs);
    });

    test('voided transaction hides the pencil but keeps the note text visible', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);
        await chargeWithNote(page, '1.50', 'doomed');

        const firstRow = page.locator('[data-testid="transactions-list"] .list-row').first();
        page.once('dialog', (d) => d.accept());
        await firstRow.locator('[data-testid="txn-void"]').click();

        // After void: note text remains, pencil and X disappear.
        await expect(firstRow.locator('[data-testid="txn-note-text"]')).toContainText('doomed');
        await expect(firstRow.locator('[data-testid="txn-note-edit"]')).toHaveCount(0);
        await expect(firstRow.locator('[data-testid="txn-void"]')).toHaveCount(0);
        assertCleanConsole(msgs);
    });
});
