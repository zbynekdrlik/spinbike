import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Card search — keyboard navigation', () => {
    test('auto-select + arrow keys work on first AND second search', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff');
        const searchInput = page.locator('input[type="search"]');
        await searchInput.waitFor();

        // --- First search: "TestCorp" returns 2 results (Jana, Petr) ---
        await searchInput.focus();
        await page.keyboard.type('TestCorp', { delay: 30 });

        // Wait for the debounced fetch to populate the dropdown.
        await expect(page.locator('[data-testid="search-result"]')).toHaveCount(2, { timeout: 3000 });

        // First result must be auto-highlighted (no click, no arrow needed).
        const firstRow = page.locator('[data-testid="search-result"]').nth(0);
        await expect(firstRow).toHaveClass(/search-result-active/);

        // Enter picks the highlighted (first) card. Jana Testova sorts first
        // alphabetically by last_name; the backend orders by last_name asc.
        await page.keyboard.press('Enter');
        const panel = page.locator('[data-testid="action-panel"]');
        await expect(panel).toBeVisible();
        await expect(panel).toContainText('Testova');

        // Close the action panel (× button, title="close").
        await panel.locator('button[title="close"]').click();
        await expect(panel).toHaveCount(0);

        // --- Second search: "TestCorp" again, exercising the regression ---
        // After the first pick_card, the input must still (or again) have focus
        // so the user can just start typing.
        await expect(searchInput).toBeFocused();
        await page.keyboard.type('TestCorp', { delay: 30 });

        await expect(page.locator('[data-testid="search-result"]')).toHaveCount(2, { timeout: 3000 });

        // The regression check: first row auto-highlighted on the SECOND search too.
        await expect(page.locator('[data-testid="search-result"]').nth(0)).toHaveClass(/search-result-active/);

        // ArrowDown moves highlight to the second row.
        await page.keyboard.press('ArrowDown');
        await expect(page.locator('[data-testid="search-result"]').nth(1)).toHaveClass(/search-result-active/);
        await expect(page.locator('[data-testid="search-result"]').nth(0)).not.toHaveClass(/search-result-active/);

        // Enter picks the second card (Petr Vzorny).
        await page.keyboard.press('Enter');
        await expect(panel).toBeVisible();
        await expect(panel).toContainText('Vzorny');

        assertCleanConsole(consoleMessages);
    });
});
