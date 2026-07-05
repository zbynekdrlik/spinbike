import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI, createUniqueUser } from './helpers';

const BASE_URL = 'http://localhost:8099';

// Clicks the already-visible action panel's "Edit Info" button. Assumes the
// panel (and its selected card) is already showing — use this for a REOPEN
// so the EditInfoForm component stays mounted (no page navigation, no
// `selected` change), as opposed to `openEditInfoSheet` below which starts
// from a fresh page load and a fresh search/select.
async function clickEditInfo(page: import('@playwright/test').Page) {
    await page
        .locator('[data-testid="action-panel"] button')
        .filter({ hasText: /edit.info|upravit/i })
        .click();
    await expect(page.locator('[data-testid="sheet-edit-info"]')).toBeVisible();
}

async function openEditInfoSheet(page: import('@playwright/test').Page, searchTerm: string) {
    await page.goto('/staff');
    const searchInput = page.locator('input[type="search"]');
    await searchInput.waitFor();
    await searchInput.focus();
    await page.keyboard.type(searchTerm, { delay: 30 });
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
    await clickEditInfo(page);
}

test.describe('Staff "Send invite" button in edit-info form (#111)', () => {
    test('disabled for a card with no saved email', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        const user = await createUniqueUser(adminToken, 0, 'NoEmail');
        await openEditInfoSheet(page, user.name);

        const inviteButton = page.locator('[data-testid="user-edit-send-invite"]');
        await expect(inviteButton).toBeVisible();
        await expect(inviteButton).toBeDisabled();

        assertCleanConsole(consoleMessages);
    });

    test('enabled for a card with a saved email; click sends the invite and closes the sheet', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        const suffix = Array.from({ length: 8 }, () =>
            String.fromCharCode(97 + Math.floor(Math.random() * 26)),
        ).join('');
        const email = `hasemail-${suffix}@test.local`;
        const user = await createUniqueUser(adminToken, 0, 'HasEmail', email);
        await openEditInfoSheet(page, user.name);

        const inviteButton = page.locator('[data-testid="user-edit-send-invite"]');
        await expect(inviteButton).toBeVisible();
        await expect(inviteButton).toBeEnabled();

        await inviteButton.click();

        // The sheet closes on invite completion (success or error) — see
        // edit_info_form.rs — specifically so this confirmation is NOT stuck
        // behind the sheet's own full-viewport backdrop blur (z-index above
        // the alert), which would make it invisible to a real user even
        // though it's technically present in the DOM.
        await expect(page.locator('[data-testid="sheet-edit-info"]')).not.toBeVisible({
            timeout: 5000,
        });

        const success = page.locator('.alert-success');
        await expect(success).toBeVisible({ timeout: 10000 });
        await expect(success).toContainText(email);

        assertCleanConsole(consoleMessages);
    });

    test('typing an email and saving enables the invite button on the next open', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        const user = await createUniqueUser(adminToken, 0, 'TypeSave');
        await openEditInfoSheet(page, user.name);
        await expect(page.locator('[data-testid="user-edit-send-invite"]')).toBeDisabled();

        const suffix = Array.from({ length: 8 }, () =>
            String.fromCharCode(97 + Math.floor(Math.random() * 26)),
        ).join('');
        const typedEmail = `typesave-${suffix}@test.local`;
        await page
            .locator('[data-testid="sheet-edit-info"] input[type="email"]')
            .fill(typedEmail);
        await page
            .locator('[data-testid="sheet-edit-info"] button[type="submit"]')
            .click();

        // Save success closes the sheet (existing behavior).
        await expect(page.locator('[data-testid="sheet-edit-info"]')).not.toBeVisible({
            timeout: 5000,
        });

        // Reopen for the SAME user — the acceptance criterion from #111:
        // "after the owner saves a newly-typed email, the button becomes
        // available".
        await openEditInfoSheet(page, user.name);
        await expect(page.locator('[data-testid="user-edit-send-invite"]')).toBeEnabled();

        assertCleanConsole(consoleMessages);
    });

    test('Cancel-then-reopen re-syncs the button against an out-of-band email change', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        const user = await createUniqueUser(adminToken, 0, 'OutOfBand');
        await openEditInfoSheet(page, user.name);
        await expect(page.locator('[data-testid="user-edit-send-invite"]')).toBeDisabled();

        // Close via Cancel — the sheet hides but the EditInfoForm component
        // itself stays mounted (only a remount, e.g. after a save that
        // changes `selected`, would otherwise re-seed the saved-email state;
        // Cancel does neither). This is the scenario the refresh Effect's
        // `set_saved_email` sync (added in review) specifically covers.
        await page
            .locator('[data-testid="sheet-edit-info"] button')
            .filter({ hasText: /zrusit|cancel/i })
            .click();
        await expect(page.locator('[data-testid="sheet-edit-info"]')).not.toBeVisible({
            timeout: 2000,
        });

        // Simulate a DIFFERENT staff terminal adding the email directly via
        // the API, bypassing this browser's form entirely.
        const suffix = Array.from({ length: 8 }, () =>
            String.fromCharCode(97 + Math.floor(Math.random() * 26)),
        ).join('');
        const outOfBandEmail = `outofband-${suffix}@test.local`;
        const putResp = await fetch(`${BASE_URL}/api/users/${user.user_id}`, {
            method: 'PUT',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
            body: JSON.stringify({ email: outOfBandEmail }),
        });
        if (!putResp.ok) {
            throw new Error(`out-of-band PUT failed: ${putResp.status} ${await putResp.text()}`);
        }

        // Reopen the SAME still-mounted EditInfoForm — NOT via a fresh
        // page.goto/search (that would reload the whole app and trivially
        // re-seed everything, masking the bug this test targets). The action
        // panel for this card is still showing after Cancel; just re-click
        // "Edit Info" so the show=false→true refresh Effect fires against
        // the SAME component instance.
        await clickEditInfo(page);
        await expect(page.locator('[data-testid="user-edit-send-invite"]')).toBeEnabled({
            timeout: 5000,
        });

        assertCleanConsole(consoleMessages);
    });
});
