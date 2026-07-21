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
    return (await r.json()) as { email?: string | null; allow_self_entry?: boolean };
}

// #141: sending an invite from the edit sheet used to take multiple steps —
// type the email, Save (sheet closes), REOPEN, only then is the invite button
// enabled. The fix makes the button (a) enable the instant a valid email is
// typed, and (b) on click, PERSIST the typed email first and THEN invite, in a
// single click. A save-step failure (e.g. the 409 email-uniqueness conflict)
// stops before inviting and shows the in-sheet error, sheet staying open.
//
// #232: the invite click used to close the sheet UNCONDITIONALLY on BOTH
// success and error, with no in-sheet confirmation — the operator couldn't
// tell the email had actually been saved and had to reopen the sheet just to
// also tick "allow self entry" and Save again. The fix keeps the sheet OPEN
// on both outcomes: success shows an in-sheet green confirmation (button
// renamed "Ulozit a poslat pozvanku" / "Save & send invite" to say so),
// error shows the same in-sheet red alert Save failures already use.
test.describe('Staff "Send invite" button in edit-info form (#111, #141, #232)', () => {
    test('disabled when the email field is empty', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        const user = await createUniqueUser(adminToken, 0, 'NoEmail');
        await openEditInfoSheet(page, user.name);

        const inviteButton = page.locator('[data-testid="user-edit-send-invite"]');
        await expect(inviteButton).toBeVisible();
        await expect(inviteButton).toBeDisabled();
        // #232: renamed to communicate the existing save-then-invite
        // semantics (#141) — one click saves the WHOLE form, not just email.
        await expect(inviteButton).toHaveText(/save.*invite/i);

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

    test('one click on a freshly-typed email + checkbox PERSISTS BOTH, sends the invite, and the sheet STAYS OPEN with an in-sheet confirmation (#141, #232)', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        const user = await createUniqueUser(adminToken, 0, 'OneClick');
        await openEditInfoSheet(page, user.name);

        // Confirm the user starts with NO email and the checkbox unticked.
        const before = await lookupCard(adminToken, user.card_code);
        expect(before.email ?? '').toBe('');
        expect(before.allow_self_entry ?? false).toBe(false);

        const typedEmail = randomEmail('oneclick');
        await page
            .locator('[data-testid="sheet-edit-info"] input[type="email"]')
            .fill(typedEmail);
        // Tick allow_self_entry in the SAME sheet session — proves the
        // one-click invite persists the WHOLE form, not just the email
        // (this is exactly the reopen-toggle-save loop #232 complained
        // about: the operator no longer needs a separate Save for this).
        await page.locator('[data-testid="user-edit-allow-self-entry"]').check();

        const inviteButton = page.locator('[data-testid="user-edit-send-invite"]');
        await expect(inviteButton).toBeEnabled();
        await inviteButton.click();

        // #232: the sheet STAYS OPEN on invite success (it used to close
        // unconditionally with no in-sheet confirmation). The in-sheet green
        // alert shows the new email; the shared dashboard channel is unused.
        const sheet = page.locator('[data-testid="sheet-edit-info"]');
        await expect(sheet).toBeVisible();
        const inSheetOk = sheet.locator('[data-testid="edit-info-invite-sent"]');
        await expect(inSheetOk).toBeVisible({ timeout: 10000 });
        await expect(inSheetOk).toContainText(typedEmail);
        // (Not asserting a bare `.alert-success` count here: the in-sheet
        // confirmation reuses that same CSS class for visual consistency
        // with the shared dashboard alert — `edit-info-invite-sent` above
        // is the precise, scoped check that it's the IN-SHEET one.)

        // The SAVE half must have committed the typed email AND the checkbox —
        // this is the part that previously required a separate Save + reopen.
        await expect
            .poll(async () => (await lookupCard(adminToken, user.card_code)).email ?? '', {
                timeout: 5000,
            })
            .toBe(typedEmail);
        expect((await lookupCard(adminToken, user.card_code)).allow_self_entry).toBe(true);

        // Close via Save (the terminal action) and reopen — proves the
        // combined save survives a full close/reopen cycle.
        await sheet.locator('button[type="submit"]').click();
        await expect(sheet).not.toBeVisible({ timeout: 5000 });
        await clickEditInfo(page);
        await expect(
            page.locator('[data-testid="sheet-edit-info"] input[type="email"]'),
        ).toHaveValue(typedEmail);
        await expect(page.locator('[data-testid="user-edit-allow-self-entry"]')).toBeChecked();

        assertCleanConsole(consoleMessages);
    });

    test('card with a saved email: click sends the invite and the sheet STAYS OPEN with an in-sheet confirmation (#232)', async ({
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

        const sheet = page.locator('[data-testid="sheet-edit-info"]');
        await expect(sheet).toBeVisible();
        const inSheetOk = sheet.locator('[data-testid="edit-info-invite-sent"]');
        await expect(inSheetOk).toBeVisible({ timeout: 10000 });
        await expect(inSheetOk).toContainText(email);

        assertCleanConsole(consoleMessages);
    });

    // #232: previously `on_close_invite.run(())` ran on BOTH success AND
    // error, so an invite failure (e.g. mail not configured) also silently
    // closed the sheet with the error routed to the shared dashboard alert
    // (occluded by the sheet's own backdrop blur while it was still
    // visible). Now invite errors route to the SAME in-sheet red alert Save
    // failures use, and the sheet stays open so the operator can retry.
    test('invite endpoint failure keeps the sheet open with the in-sheet red alert (#232)', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        const email = randomEmail('inviteerr');
        const user = await createUniqueUser(adminToken, 0, 'InviteErr', email);
        await openEditInfoSheet(page, user.name);

        // The save step (PUT) succeeds normally; only the invite POST fails.
        // Status kept in the 4xx range so the console-check helper's existing
        // benign-4xx filter applies (a mocked failure isn't a real bug) —
        // the client branches on the JSON `error` string, not the status.
        await page.route(`**/api/users/${user.user_id}/invite`, (route) =>
            route.fulfill({
                status: 400,
                contentType: 'application/json',
                body: JSON.stringify({ error: 'mail_not_configured' }),
            }),
        );

        const inviteButton = page.locator('[data-testid="user-edit-send-invite"]');
        await expect(inviteButton).toBeEnabled();
        await inviteButton.click();

        const sheet = page.locator('[data-testid="sheet-edit-info"]');
        await expect(sheet).toBeVisible();
        const inSheetError = sheet.locator('[data-testid="edit-info-error"]');
        await expect(inSheetError).toBeVisible({ timeout: 5000 });
        await expect(page.locator('[data-testid="edit-info-invite-sent"]')).toHaveCount(0);
        await expect(page.locator('.alert-success')).toHaveCount(0);

        // The save half still committed — the failure is invite-only.
        expect((await lookupCard(adminToken, user.card_code)).email ?? '').toBe(email);

        assertCleanConsole(consoleMessages);
    });

    // #232 (code-review finding): the action-panel's own "Edit info" button
    // both OPENS and CLOSES the sheet (a plain toggle in card_panel.rs) —
    // activating it again while the sheet is open bypasses EditInfoForm's
    // own Cancel button AND the Sheet's backdrop/Escape handler entirely (it
    // just flips `show_edit` directly). A MOUSE click can't actually reach
    // it — the Sheet's full-viewport `.sheet-backdrop` visually covers it
    // and intercepts pointer events — but the Sheet has no keyboard focus
    // trap, so a keyboard user can still Tab back to the (still-focusable,
    // still-in-the-DOM) button and press Enter to activate it; hence
    // `.focus()` + Enter below rather than `.click()`. A first version of
    // this fix only flushed the post-invite stash at the two mouse-reachable
    // close points and missed this THIRD way of closing — fixed by flushing
    // centrally off the `show` signal itself instead of enumerating buttons.
    test('closing via the keyboard-activated "Edit info" toggle button (not Cancel/backdrop) still flushes a post-invite name change to the dashboard (#232)', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        const email = randomEmail('toggleclose');
        const user = await createUniqueUser(adminToken, 0, 'ToggleClose', email);
        await openEditInfoSheet(page, user.name);

        const newName = `${user.name} Edited`;
        const sheet = page.locator('[data-testid="sheet-edit-info"]');
        await sheet.locator('input[type="text"]').first().fill(newName);

        const inviteButton = page.locator('[data-testid="user-edit-send-invite"]');
        await expect(inviteButton).toBeEnabled();
        await inviteButton.click();
        await expect(sheet.locator('[data-testid="edit-info-invite-sent"]')).toBeVisible({
            timeout: 10000,
        });

        // Activate the SAME "Edit info" button that OPENED the sheet via
        // keyboard (Enter on the focused element) — a `.click()` here would
        // fail: the sheet backdrop intercepts pointer events over it.
        const editInfoButton = page
            .locator('[data-testid="action-panel"] button')
            .filter({ hasText: /edit.info|upravit/i });
        await editInfoButton.focus();
        await page.keyboard.press('Enter');
        await expect(sheet).not.toBeVisible({ timeout: 5000 });

        // The action-panel header renders `name` from the `card` prop it was
        // mounted with (mod.rs's `match selected.get()`) — it only reflects
        // the invite-time name change if the stash was flushed to
        // `set_selected` on THIS close path too, not just Cancel/backdrop.
        await expect(page.locator('[data-testid="action-panel"] .card-title__name')).toHaveText(
            newName,
        );

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
