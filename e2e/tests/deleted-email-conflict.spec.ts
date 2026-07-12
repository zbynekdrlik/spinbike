import { test, expect } from '@playwright/test';
import {
    setupConsoleCheck,
    assertCleanConsole,
    setEnglishLanguage,
    loginViaAPI,
    createUniqueUser,
} from './helpers';

const BASE_URL = 'http://localhost:8099';
const RUN_TAG = `DEC${Math.random().toString(36).slice(2, 6).toUpperCase()}`;

// #143 — reusing an email held by a SOFT-DELETED account must show the staff
// resolution dialog (not an opaque error), and the "Free the email" action must
// resolve it and let the original create complete.
test.describe('Soft-deleted email conflict resolution (#143)', () => {
    test('free-email resolves the conflict and the create then succeeds', async ({
        page,
        baseURL,
    }) => {
        const messages = setupConsoleCheck(page);
        const token = await loginViaAPI(page, baseURL!, 'staff@test.com', 'staff123');
        await setEnglishLanguage(page);

        // 1) Create an old account holding a unique email, then soft-delete it —
        // the email is now "locked" by the archived row.
        const email = `deleted.${RUN_TAG.toLowerCase()}@e2e.local`;
        const old = await createUniqueUser(token, 0, 'DEC', email);
        const del = await fetch(`${BASE_URL}/api/users/${old.user_id}`, {
            method: 'DELETE',
            headers: { Authorization: `Bearer ${token}` },
        });
        expect(del.ok).toBeTruthy();

        // 2) Try to add a NEW person with that same (soft-deleted) email.
        await page.goto('/staff');
        await page.getByRole('button', { name: /add person/i }).click();
        await page.getByLabel(/name/i).fill(`New Person ${RUN_TAG}`);
        await page.getByLabel(/email/i).fill(email);
        await page.getByTestId('add-person-submit').click();

        // 3) The resolution dialog appears, naming the archived account (a clear
        // message + explicit actions — NOT a dead-end 500/opaque error).
        const dialog = page.getByTestId('sheet-deleted-email-conflict');
        await expect(dialog).toBeVisible({ timeout: 5000 });
        await expect(page.getByTestId('deleted-email-conflict-body')).toContainText(
            old.name,
            { timeout: 2000 },
        );

        // 4) "Free the email" → the create auto-retries and succeeds.
        await page.getByTestId('deleted-email-free').click();
        await expect(page.locator('.alert-success')).toContainText('Person added', {
            timeout: 5000,
        });
        await expect(dialog).toBeHidden();

        assertCleanConsole(messages);
    });
});
