import { test, expect, Page } from '@playwright/test';
import {
    setupConsoleCheck,
    assertCleanConsole,
    loginViaAPI,
    createUniqueUser,
} from './helpers';

const BASE_URL = 'http://localhost:8099';

// #71: desk_reset signal — clicking nav-desk or brand-link while already
// on /staff must clear the open card / search query and return the
// dashboard to its idle state. No E2E covered this before; this spec
// pins both surfaces.

async function searchAndOpenCard(page: Page, term: string) {
    const search = page.locator('input[type="search"]');
    await search.waitFor();
    await search.focus();
    await page.keyboard.type(term, { delay: 30 });
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
}

test.describe('Dashboard desk-reset (#71)', () => {
    test('nav-desk click clears the open card', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        const u = await createUniqueUser(token, 0.0, 'DR-NAV');

        await page.goto('/staff');
        await searchAndOpenCard(page, u.card_code);

        // Pre-state: action panel visible (search input may have been
        // cleared by the result-pick flow — that's a search-UX detail
        // unrelated to the desk-reset signal under test).
        await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

        // Click the Desk nav while already on /staff.
        await page.locator('[data-testid="nav-desk"]').click();

        // Action panel must vanish; search must be empty.
        await expect(page.locator('[data-testid="action-panel"]')).toBeHidden();
        await expect(page.locator('input[type="search"]')).toHaveValue('');

        assertCleanConsole(msgs);
    });

    test('brand-link click clears the open card', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        const u = await createUniqueUser(token, 0.0, 'DR-BRD');

        await page.goto('/staff');
        await searchAndOpenCard(page, u.card_code);

        await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

        // Brand link is the staff equivalent of "go home" while on /staff.
        await page.locator('[data-testid="brand-link"]').click();

        await expect(page.locator('[data-testid="action-panel"]')).toBeHidden();
        await expect(page.locator('input[type="search"]')).toHaveValue('');

        assertCleanConsole(msgs);
    });
});
