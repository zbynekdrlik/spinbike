import { test, expect } from '@playwright/test';
import { loginViaUI, setupConsoleCheck, assertCleanConsole } from './helpers';

test('Now panel renders on Desk', async ({ page }) => {
    const consoleMessages = setupConsoleCheck(page);
    await loginViaUI(page, 'admin@test.com', 'admin123');
    await page.goto('/staff');
    await expect(page.locator('[data-testid="now-panel"]')).toBeVisible();
    // At least one of the three head variants must appear.
    const heads = page.locator(
        '[data-testid="now-panel-head-running"], [data-testid="now-panel-head-next"], [data-testid="now-panel-head-empty"]'
    );
    await expect(heads).toHaveCount(1);
    assertCleanConsole(consoleMessages);
});
