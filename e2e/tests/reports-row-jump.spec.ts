import { test, expect } from '@playwright/test';
import {
    loginViaAPI,
    createUniqueUser,
    setupConsoleCheck,
    assertCleanConsole,
} from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Reports row → card panel direct jump', () => {
    test('clicking a feed row with a barcode opens the card panel directly without dropdown', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        // Log in admin and grab the token for seed API calls.
        const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        // Seed a user with a unique card_code and initial credit > 0 so that the
        // creation creates a topup transaction, which lands in today's report
        // feed. createUniqueUser returns { user_id, name, card_code }.
        const { name, card_code } = await createUniqueUser(token, 12.34, 'JUMP');
        // Extract the searchable part: name = "JUMP JUMP${suffix}", lastName is "JUMP${suffix}"
        const lastName = name.split(' ').slice(1).join('');
        const barcode = card_code;

        // Navigate to /reports — the today (day) view is the default.
        await page.goto(`${BASE_URL}/reports`);
        await expect(page.locator('[data-testid="reports-page"]')).toBeVisible({ timeout: 10000 });

        // Find the feed-row for our seeded card by matching the rendered
        // card_name, which is "<first_name> <last_name>" = "JUMP <lastName>".
        // We match on lastName alone because it already contains the unique
        // suffix; no other card in the DB can share it.
        const seededRow = page
            .locator('[data-testid="feed-row"]')
            .filter({ hasText: lastName })
            .first();
        await expect(seededRow).toBeVisible({ timeout: 10000 });
        await expect(seededRow).toHaveClass(/list-row--interactive/);

        // Click → URL changes to /staff?card=<barcode>
        await seededRow.click();
        await page.waitForURL(/\/staff\?card=/, { timeout: 5000 });

        // The URL must contain our exact barcode (URL-encoded).
        expect(page.url()).toContain(encodeURIComponent(barcode));

        // Card action panel renders directly — no dropdown step required.
        await expect(page.locator('[data-testid="action-panel"]')).toBeVisible({ timeout: 10000 });

        // Search dropdown is NOT visible (we skipped it).
        await expect(page.locator('[data-testid="search-result"]')).toHaveCount(0);

        assertCleanConsole(consoleMessages);
    });
});
