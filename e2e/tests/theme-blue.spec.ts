import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole } from './helpers';

const BASE_URL = 'http://localhost:8099';

// Reads the resolved value of a CSS custom property on :root.
async function readBrand(page: import('@playwright/test').Page): Promise<string> {
    return await page.evaluate(() =>
        getComputedStyle(document.documentElement).getPropertyValue('--brand').trim()
    );
}

// Convert "#60a5fa" or "rgb(96, 165, 250)" → [R, G, B].
function parseColor(s: string): [number, number, number] {
    const hex = s.match(/^#([0-9a-fA-F]{6})$/);
    if (hex) {
        const n = parseInt(hex[1], 16);
        return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
    }
    const rgb = s.match(/rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/);
    if (rgb) return [parseInt(rgb[1], 10), parseInt(rgb[2], 10), parseInt(rgb[3], 10)];
    throw new Error(`unparseable color: ${s}`);
}

function near(a: number, b: number, tol = 4): boolean {
    return Math.abs(a - b) <= tol;
}

test.describe('Theme — vibrant blue brand token', () => {
    test('dark mode: --brand resolves to ≈ #60a5fa', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        await page.emulateMedia({ colorScheme: 'dark' });
        // /login renders without auth and produces a styled :root.
        await page.goto(`${BASE_URL}/login`);
        await page.waitForSelector('button[type="submit"]');

        const raw = await readBrand(page);
        const [r, g, b] = parseColor(raw);
        // #60a5fa = (96, 165, 250)
        expect(near(r, 96)).toBe(true);
        expect(near(g, 165)).toBe(true);
        expect(near(b, 250)).toBe(true);

        assertCleanConsole(msgs);
    });

    test('light mode: --brand resolves to ≈ #2563eb', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        await page.emulateMedia({ colorScheme: 'light' });
        await page.goto(`${BASE_URL}/login`);
        await page.waitForSelector('button[type="submit"]');

        const raw = await readBrand(page);
        const [r, g, b] = parseColor(raw);
        // #2563eb = (37, 99, 235)
        expect(near(r, 37)).toBe(true);
        expect(near(g, 99)).toBe(true);
        expect(near(b, 235)).toBe(true);

        assertCleanConsole(msgs);
    });
});
