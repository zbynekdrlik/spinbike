import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole, setIosStandalone } from './helpers';

const BASE_URL = 'http://localhost:8099';

/**
 * #386: on an installed iPhone PWA the admin got trapped behind the "Viac"
 * (More) sheet / on /settings. An iPhone has no Escape key and a standalone
 * PWA has no browser back button, so EVERY overlay and every sub-page needs
 * an explicit, on-screen way out that does not depend on one particular
 * WebKit tap behaviour (the backdrop tap).
 *
 * Runs ONLY in the `webkit-iphone` Playwright project (real WebKit engine +
 * the iPhone 13 descriptor: touch, mobile viewport, iOS user agent) — see
 * playwright.config.ts. Every interaction is a `tap()`, the way a finger
 * reaches the app.
 */
test.describe('iPhone PWA — every overlay and sub-page has a way out (#386)', () => {
    test('More sheet closes via its close button and backdrop; /settings has a back control', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await setIosStandalone(page);
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        await page.goto('/staff');
        const more = page.locator('[data-testid="nav-more"]');
        await expect(more).toBeVisible();

        // 1. Viac -> Nastavenia lands on /settings with the sheet GONE and
        //    the bottom nav usable again.
        await more.tap();
        await expect(page.locator('[data-testid="more-sheet"]')).toBeVisible();
        await page.locator('[data-testid="more-settings"]').tap();
        await expect(page).toHaveURL(/\/settings$/);
        await expect(page.locator('.sheet-backdrop')).toHaveCount(0);
        await expect(page.locator('[data-testid="nav-desk"]')).toBeVisible();

        // 2. The sheet carries its own visible close button that closes it.
        await more.tap();
        await expect(page.locator('[data-testid="more-sheet"]')).toBeVisible();
        const close = page.locator('[data-testid="more-sheet"] [data-testid="sheet-close"]');
        await expect(close).toBeVisible();
        await close.tap();
        await expect(page.locator('[data-testid="more-sheet"]')).toHaveCount(0);
        await expect(page.locator('.sheet-backdrop')).toHaveCount(0);

        // 3. A tap on the backdrop (the dimmed area ABOVE the bottom sheet)
        //    closes it too, under the real WebKit engine.
        await more.tap();
        await expect(page.locator('[data-testid="more-sheet"]')).toBeVisible();
        await page.locator('.sheet-backdrop').tap({ position: { x: 20, y: 20 } });
        await expect(page.locator('[data-testid="more-sheet"]')).toHaveCount(0);
        await expect(page.locator('.sheet-backdrop')).toHaveCount(0);

        // 4. The Sluzby (services) table scrolls inside its own wrapper
        //    instead of widening the whole page past the phone screen
        //    (a horizontally-draggable page can displace the fixed nav).
        await page.locator('[data-testid="admin-tab-services"]').tap();
        const tableScroll = page.locator('[data-testid="services-table-scroll"]');
        await expect(tableScroll).toBeVisible();
        await expect(tableScroll).toHaveCSS('overflow-x', 'auto');
        const pageOverflow = await page.evaluate(
            () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
        );
        expect(pageOverflow, 'the /settings page must not scroll horizontally on a phone').toBeLessThanOrEqual(0);

        // 5. /settings has its own back control in the page header — the
        //    standalone PWA has no browser back button.
        const back = page.locator('[data-testid="settings-back"]');
        await expect(back).toBeVisible();
        await back.tap();
        await expect(page).toHaveURL(/\/staff$/);

        assertCleanConsole(consoleMessages);
    });
});
