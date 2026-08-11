import { test, expect, Page } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI, uniqueLetterSuffix } from './helpers';

const BASE_URL = 'http://localhost:8099';

async function createUniqueUser(
    token: string,
    initialCredit: number,
): Promise<{ card_code: string; lastName: string }> {
    // Letters-only suffix (#39 collision class — see helpers.ts) so this
    // card_code can never substring-collide with another spec's short
    // digit search in the shared, single-server E2E DB.
    const suffix = uniqueLetterSuffix();
    const cardCode = `NOTE-${suffix}`;
    const lastName = `Note${suffix}`;
    const resp = await fetch(`${BASE_URL}/api/users`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ name: `NT ${lastName}`, initial_credit: initialCredit, card_code: cardCode }),
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
        const { lastName } = await createUniqueUser(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);
        await chargeWithNote(page, '2.50', 'Proteinová tyčinka');

        const noteRow = page.locator('[data-testid="txn-note-text"]').first();
        await expect(noteRow).toBeVisible();
        await expect(noteRow).toContainText('Proteinová tyčinka');

        assertCleanConsole(msgs);
    });

    test('note appears on report activity feed', async ({ page }) => {
        // /api/reports/day is admin-only — log in as admin@test.com (matches
        // the pattern in reports-attendance.spec.ts). admin can also drive the
        // staff dashboard to create the seeded charge.
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        const { lastName } = await createUniqueUser(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);
        const noteText = `feed-${Date.now()}`;
        await chargeWithNote(page, '1.00', noteText);

        // Wait for the report fetch to land before asserting on the DOM.
        const reportResp = page.waitForResponse(
            (r) => r.url().includes('/api/reports/day') && r.request().method() === 'GET',
        );
        await page.goto('/reports');
        await reportResp;

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
        const { lastName } = await createUniqueUser(token, 50.0);
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
        const { lastName } = await createUniqueUser(token, 50.0);
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
        const { lastName } = await createUniqueUser(token, 50.0);
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
        const { lastName } = await createUniqueUser(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);
        await chargeWithNote(page, '1.50', 'doomed');

        const firstRow = page.locator('[data-testid="transactions-list"] .list-row').first();
        // #291: unlike every sibling test in this file (which waits for its
        // OWN mutation's response before asserting on the DOM), this test
        // used to click void and assert immediately with no synchronization
        // at all -- racing Playwright's own expect() auto-retry against the
        // full DELETE -> txn_refresh bump -> GET refetch -> re-render chain,
        // observed flaking on CI (element(s) not found on the note-text
        // locator, CI runs 31197701928 and 31203717764). Waiting for the
        // DELETE response explicitly (matching the PATCH-response pattern
        // the 'inline pencil edits'/'clearing a note' tests above already
        // use successfully) proves the void itself landed before the
        // remaining refetch+render latency is left to expect()'s own retry
        // loop, same as those already-reliable tests.
        const voidResp = page.waitForResponse(
            (r) => /\/api\/transactions\/\d+$/.test(r.url()) && r.request().method() === 'DELETE',
        );
        page.once('dialog', (d) => d.accept());
        await firstRow.locator('[data-testid="txn-void"]').click();
        const resp = await voidResp;
        expect(resp.ok()).toBe(true);

        // After void: note text remains, pencil and X disappear.
        await expect(firstRow.locator('[data-testid="txn-note-text"]')).toContainText('doomed');
        await expect(firstRow.locator('[data-testid="txn-note-edit"]')).toHaveCount(0);
        await expect(firstRow.locator('[data-testid="txn-void"]')).toHaveCount(0);
        assertCleanConsole(msgs);
    });

    // #344 finding 2: per-row `editing`/`note_value` signals are created
    // INSIDE the reactive block that re-runs on ANY txn_refresh bump — void'ing
    // (or re-dating, or saving a note on) one row used to tear down and
    // recreate every OTHER row's signals too, silently discarding an
    // in-progress, unsaved note edit on an unrelated row.
    test('an unrelated row action does not discard an unsaved, still-open note edit on another row', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await createUniqueUser(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);
        // Two transactions: rows render newest-first, so after both charges
        // row[0] = "second" (most recent) and row[1] = "first" (older).
        await chargeWithNote(page, '1.00', 'first');
        await chargeWithNote(page, '1.00', 'second');

        const rows = page.locator('[data-testid="transaction-row"]');
        const olderRow = rows.nth(1);
        const newerRow = rows.nth(0);
        await expect(olderRow.locator('[data-testid="txn-note-text"]')).toContainText('first');
        await expect(newerRow.locator('[data-testid="txn-note-text"]')).toContainText('second');

        // Start editing the OLDER row's note but do NOT save.
        await olderRow.locator('[data-testid="txn-note-edit"]').click();
        const editInput = olderRow.locator('[data-testid="txn-note-edit-input"]');
        await expect(editInput).toBeVisible();
        await editInput.fill('UNSAVED DRAFT');

        // Void the OTHER (newer) row — this bumps txn_refresh and re-fetches,
        // re-running the whole rows block.
        const voidResp = page.waitForResponse(
            (r) => /\/api\/transactions\/\d+$/.test(r.url()) && r.request().method() === 'DELETE',
        );
        page.once('dialog', (d) => d.accept());
        await newerRow.locator('[data-testid="txn-void"]').click();
        const resp = await voidResp;
        expect(resp.ok()).toBe(true);

        // The older row's editor must still be open with the unsaved text —
        // not reverted, not closed.
        await expect(editInput).toBeVisible();
        await expect(editInput).toHaveValue('UNSAVED DRAFT');

        assertCleanConsole(msgs);
    });
});
