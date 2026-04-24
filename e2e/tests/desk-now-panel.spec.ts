import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole } from './helpers';

const BASE_URL = 'http://localhost:8099';

test('Now panel renders on Desk', async ({ page }) => {
    const consoleMessages = setupConsoleCheck(page);
    await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
    await page.goto('/staff');
    await expect(page.locator('[data-testid="now-panel"]')).toBeVisible();
    const heads = page.locator(
        '[data-testid="now-panel-head-running"], [data-testid="now-panel-head-next"], [data-testid="now-panel-head-empty"]'
    );
    await expect(heads).toHaveCount(1);
    assertCleanConsole(consoleMessages);
});
