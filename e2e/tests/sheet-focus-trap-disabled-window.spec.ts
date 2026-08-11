import { test, expect, Page } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI, createUniqueUser } from './helpers';

/**
 * #334 — follow-up on #320's `Sheet` Tab-cycle focus trap
 * (`spinbike-ui/src/components/sheet.rs`). `trap_tab` only ever runs once a
 * `keydown` bubbles through a descendant that is still inside `.sheet`. On
 * 2 of the 8 `Sheet` call sites — `dashboard/sheets/delete_user.rs` (Cancel
 * + Delete) and `dashboard/deleted_email_conflict.rs` (Restore + Free +
 * Cancel) — EVERY focusable control shares one busy/saving signal. The
 * instant that signal flips `true` mid-request, every control becomes
 * `disabled`, `FOCUSABLE_SELECTOR` matches ZERO elements, and the browser
 * auto-blurs the just-disabled (previously focused) control to `<body>` —
 * BEFORE any keydown can ever reach `trap_tab`'s handler. During that
 * window Tab could carry focus into page content behind the backdrop.
 *
 * This spec drives `DeleteUserSheet` (the simpler of the two call sites)
 * and holds the DELETE response open so the transient window is directly
 * observable rather than racing a sub-second timer.
 */

const BASE_URL = 'http://localhost:8099';

async function openDeleteUserSheet(page: Page, cardCode: string) {
    await page.goto('/staff');
    await page.waitForSelector('input[type="search"]');
    await page.fill('input[type="search"]', cardCode);
    const result = page.locator('[data-testid="search-result"]').first();
    await expect(result).toBeVisible({ timeout: 3000 });
    await result.click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

    await page.locator('[data-testid="delete-user-button"]').click();
    const sheet = page.locator('[data-testid="sheet-delete-user"]');
    await expect(sheet).toBeVisible();
    return sheet;
}

test.describe('Sheet focus trap — all-descendants-disabled transient window (#334)', () => {
    test('focus returns inside the sheet, and Tab cannot escape, while every control is disabled', async ({
        page,
    }) => {
        const messages = setupConsoleCheck(page);
        const staffToken = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const user = await createUniqueUser(staffToken, 0, 'FT');

        const sheet = await openDeleteUserSheet(page, user.card_code);

        // Hold the DELETE response open so the shared `saving` signal — and
        // the resulting all-descendants-disabled window — stays observable
        // for the whole test instead of racing a sub-second real request.
        let releaseDelete: () => void = () => {};
        const deleteHeld = new Promise<void>((resolve) => {
            releaseDelete = resolve;
        });
        await page.route(`**/api/users/${user.user_id}`, async (route) => {
            if (route.request().method() !== 'DELETE') {
                await route.continue();
                return;
            }
            await deleteHeld;
            await route.fulfill({ status: 204, body: '' });
        });

        const confirmBtn = page.locator('[data-testid="delete-user-confirm"]');
        await confirmBtn.focus();
        await expect(confirmBtn).toBeFocused();

        // `saving` flips true synchronously on click, disabling BOTH
        // buttons — the just-focused Confirm button is auto-blurred by the
        // browser before the (held-open) DELETE even resolves.
        await confirmBtn.click();
        await expect(confirmBtn).toBeDisabled();
        await expect(page.locator('[data-testid="delete-user-cancel"]')).toBeDisabled();

        // The fix refocuses `.sheet` itself once the browser's own
        // auto-blur is observed — poll for it rather than a fixed sleep.
        await page.waitForFunction(
            () => {
                const sheetEl = document.querySelector('[data-testid="sheet-delete-user"]');
                return !!sheetEl && sheetEl.contains(document.activeElement);
            },
            { timeout: 1000 },
        );

        // And Tab, pressed while every descendant is still disabled, must
        // not be able to carry focus out to page content behind the
        // backdrop — there is nothing inside the sheet to move to, so it
        // MUST do nothing here.
        await page.keyboard.press('Tab');
        const stillInsideSheet = await page.evaluate(() => {
            const sheetEl = document.querySelector('[data-testid="sheet-delete-user"]');
            return !!sheetEl && sheetEl.contains(document.activeElement);
        });
        expect(stillInsideSheet).toBe(true);

        // Let the request complete and clean up — a real save afterward
        // still succeeds through the trap.
        releaseDelete();
        await expect(sheet).toBeHidden();

        assertCleanConsole(messages);
    });
});
