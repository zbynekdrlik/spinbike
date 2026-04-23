import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('redesign: adaptive theme', () => {
    test('light preference applies light background token', async ({ page }) => {
        const msgs = setupConsoleCheck(page);

        // Emulate light mode before any navigation so the CSS media query resolves correctly.
        await page.emulateMedia({ colorScheme: 'light' });
        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff');

        // Wait for the app to hydrate enough for WASM-driven styles to apply.
        await page.waitForSelector('input[type="search"]');

        // The design spec sets --color-bg to #f6f7f9 in light mode and applies it
        // to document.body. Verify computed background-color matches.
        // rgb(246, 247, 249) === #f6f7f9
        const bg = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
        expect(bg).toMatch(/rgb\(\s*246,\s*247,\s*249\s*\)/);

        assertCleanConsole(msgs);
    });

    test('dark preference applies dark background token', async ({ page }) => {
        const msgs = setupConsoleCheck(page);

        // Emulate dark mode before any navigation so the CSS media query resolves correctly.
        await page.emulateMedia({ colorScheme: 'dark' });
        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff');

        // Wait for the app to hydrate enough for WASM-driven styles to apply.
        await page.waitForSelector('input[type="search"]');

        // The design spec sets --color-bg to #0a0b0e in dark mode and applies it
        // to document.body. Verify computed background-color matches.
        // rgb(10, 11, 14) === #0a0b0e
        const bg = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
        expect(bg).toMatch(/rgb\(\s*10,\s*11,\s*14\s*\)/);

        assertCleanConsole(msgs);
    });
});
