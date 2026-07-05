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

const randomEmail = (prefix: string) => {
    const suffix = Array.from({ length: 8 }, () =>
        String.fromCharCode(97 + Math.floor(Math.random() * 26)),
    ).join('');
    return `${prefix}-${suffix}@test.local`;
};

// Fetch a card's currently-persisted state (proves the SAVE half of
// save-then-invite actually committed the email, not just that an invite fired).
async function lookupCard(token: string, cardCode: string) {
    const r = await fetch(`${BASE_URL}/api/users/lookup/${cardCode}`, {
        headers: { Authorization: `Bearer ${token}` },
    });
    if (!r.ok) throw new Error(`lookup ${cardCode}: ${r.status} ${await r.text()}`);
    return (await r.json()) as { email?: string | null };
}

// #141: sending an invite from the edit sheet used to take multiple steps —
// type the email, Save (sheet closes), REOPEN, only then is the invite button
// enabled. The fix makes the button (a) enable the instant a valid email is
// typed, and (b) on click, PERSIST the typed email first and THEN invite, in a
// single click. A save-step failure (e.g. the 409 email-uniqueness conflict)
// stops before inviting and shows the in-sheet error, sheet staying open.
test.describe('Staff "Send invite" button in edit-info form (#111, #141)', () => {
    test('disabled when the email field is empty', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        const user = await createUniqueUser(adminToken, 0, 'NoEmail');
        await openEditInfoSheet(page, user.name);

        const inviteButton = page.locator('[data-testid="user-edit-send-invite"]');
        await expect(inviteButton).toBeVisible();
        await expect(inviteButton).toBeDisabled();

        assertCleanConsole(consoleMessages);
    });

    test('typing an email ENABLES the button on the SAME open — no pre-save/reopen (#141)', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        const user = await createUniqueUser(adminToken, 0, 'TypeEnable');
        await openEditInfoSheet(page, user.name);

        const inviteButton = page.locator('[data-testid="user-edit-send-invite"]');
        await expect(inviteButton).toBeDisabled();

        // Type an email — the button must enable on THIS open, with no Save and
        // no reopen (the whole point of #141). RED before the fix (the gate was
        // keyed on the last-SAVED email, not the typed value).
        await page
            .locator('[data-testid="sheet-edit-info"] input[type="email"]')
            .fill(randomEmail('typeenable'));
        await expect(inviteButton).toBeEnabled();

        // Clearing it again disables the button (the gate tracks the live value).
        await page.locator('[data-testid="sheet-edit-info"] input[type="email"]').fill('');
        await expect(inviteButton).toBeDisabled();

        assertCleanConsole(consoleMessages);
    });

    test('one click on a freshly-typed email PERSISTS it AND sends the invite — no separate Save (#141)', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        const user = await createUniqueUser(adminToken, 0, 'OneClick');
        await openEditInfoSheet(page, user.name);

        // Confirm the user starts with NO email persisted.
        expect((await lookupCard(adminToken, user.card_code)).email ?? '').toBe('');

        const typedEmail = randomEmail('oneclick');
        await page
            .locator('[data-testid="sheet-edit-info"] input[type="email"]')
            .fill(typedEmail);

        const inviteButton = page.locator('[data-testid="user-edit-send-invite"]');
        await expect(inviteButton).toBeEnabled();
        await inviteButton.click();

        // The sheet closes on invite completion so the confirmation isn't stuck
        // behind the sheet's own full-viewport backdrop blur.
        await expect(page.locator('[data-testid="sheet-edit-info"]')).not.toBeVisible({
            timeout: 5000,
        });

        const success = page.locator('.alert-success');
        await expect(success).toBeVisible({ timeout: 10000 });
        await expect(success).toContainText(typedEmail);

        // The SAVE half must have committed the typed email — this is the part
        // that previously required a separate Save + reopen.
        await expect
            .poll(async () => (await lookupCard(adminToken, user.card_code)).email ?? '', {
                timeout: 5000,
            })
            .toBe(typedEmail);

        assertCleanConsole(consoleMessages);
    });

    test('card with a saved email: click sends the invite and closes the sheet', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        const email = randomEmail('hasemail');
        const user = await createUniqueUser(adminToken, 0, 'HasEmail', email);
        await openEditInfoSheet(page, user.name);

        const inviteButton = page.locator('[data-testid="user-edit-send-invite"]');
        await expect(inviteButton).toBeVisible();
        await expect(inviteButton).toBeEnabled();

        await inviteButton.click();

        await expect(page.locator('[data-testid="sheet-edit-info"]')).not.toBeVisible({
            timeout: 5000,
        });

        const success = page.locator('.alert-success');
        await expect(success).toBeVisible({ timeout: 10000 });
        await expect(success).toContainText(email);

        assertCleanConsole(consoleMessages);
    });

    test('a colliding typed email: one-click invite shows the in-sheet 409 error, stays open, sends nothing', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        // A holder account that already OWNS an email (and has a card_code, so
        // the staff-visible 409 names both).
        const holderEmail = randomEmail('holder');
        const holder = await createUniqueUser(adminToken, 0, 'Holder', holderEmail);

        // The victim we try to invite using the already-taken email.
        const victim = await createUniqueUser(adminToken, 0, 'Victim');
        await openEditInfoSheet(page, victim.name);

        await page
            .locator('[data-testid="sheet-edit-info"] input[type="email"]')
            .fill(holderEmail);

        const inviteButton = page.locator('[data-testid="user-edit-send-invite"]');
        await expect(inviteButton).toBeEnabled();
        await inviteButton.click();

        // The SAVE step 409s → in-sheet error, naming the colliding account, and
        // the sheet MUST stay open (fix inline). The invite must NOT fire.
        const sheet = page.locator('[data-testid="sheet-edit-info"]');
        const inSheetError = sheet.locator('[data-testid="edit-info-error"]');
        await expect(inSheetError).toBeVisible({ timeout: 5000 });
        await expect(inSheetError).toContainText(holder.name);
        await expect(sheet).toBeVisible();
        await expect(page.locator('.alert-success')).toHaveCount(0);

        // The victim must NOT have been given the colliding email.
        expect((await lookupCard(adminToken, victim.card_code)).email ?? '').toBe('');

        // The 409 logs a browser-level 4xx "Failed to load resource" that the
        // shared helper already filters — nothing else should appear.
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
        // itself stays mounted (only a remount, e.g. after a save that changes
        // `selected`, would re-seed field state; Cancel does neither). This is
        // the scenario the refresh Effect's `email_sig` sync specifically covers.
        await page
            .locator('[data-testid="sheet-edit-info"] button')
            .filter({ hasText: /zrusit|cancel/i })
            .click();
        await expect(page.locator('[data-testid="sheet-edit-info"]')).not.toBeVisible({
            timeout: 2000,
        });

        // A DIFFERENT staff terminal adds the email directly via the API,
        // bypassing this browser's form entirely.
        const outOfBandEmail = randomEmail('outofband');
        const putResp = await fetch(`${BASE_URL}/api/users/${user.user_id}`, {
            method: 'PUT',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
            body: JSON.stringify({ email: outOfBandEmail }),
        });
        if (!putResp.ok) {
            throw new Error(`out-of-band PUT failed: ${putResp.status} ${await putResp.text()}`);
        }

        // Reopen the SAME still-mounted EditInfoForm (NOT a fresh page.goto/search,
        // which would reload the app and trivially re-seed everything). The
        // show=false→true refresh Effect fires and its email smart-overwrite must
        // sync `email_sig`, re-enabling the button.
        await clickEditInfo(page);
        await expect(page.locator('[data-testid="user-edit-send-invite"]')).toBeEnabled({
            timeout: 5000,
        });

        assertCleanConsole(consoleMessages);
    });
});
