import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

function parseRgb(s: string): [number, number, number] {
    const m = s.match(/rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/);
    if (!m) throw new Error(`unparseable color: ${s}`);
    return [parseInt(m[1], 10), parseInt(m[2], 10), parseInt(m[3], 10)];
}

function near(a: number, b: number, tol = 4): boolean {
    return Math.abs(a - b) <= tol;
}

test.describe('Theme — vibrant blue', () => {
    test('dark mode: primary button background ≈ #60a5fa', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        await page.emulateMedia({ colorScheme: 'dark' });
        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff');
        await page.waitForSelector('input[type="search"]');

        const bg = await page.locator('button.btn--primary').first().evaluate(el =>
            getComputedStyle(el as HTMLElement).backgroundColor
        );
        const [r, g, b] = parseRgb(bg);
        // #60a5fa = rgb(96, 165, 250)
        expect(near(r, 96)).toBe(true);
        expect(near(g, 165)).toBe(true);
        expect(near(b, 250)).toBe(true);

        assertCleanConsole(msgs);
    });

    test('light mode: primary button background ≈ #2563eb', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        await page.emulateMedia({ colorScheme: 'light' });
        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff');
        await page.waitForSelector('input[type="search"]');

        const bg = await page.locator('button.btn--primary').first().evaluate(el =>
            getComputedStyle(el as HTMLElement).backgroundColor
        );
        const [r, g, b] = parseRgb(bg);
        // #2563eb = rgb(37, 99, 235)
        expect(near(r, 37)).toBe(true);
        expect(near(g, 99)).toBe(true);
        expect(near(b, 235)).toBe(true);

        assertCleanConsole(msgs);
    });
});
