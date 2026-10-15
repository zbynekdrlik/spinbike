import { test, expect, type Page } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole } from './helpers';

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
 * reaches the app. Each exit is its own test so a regression in one never
 * hides behind an earlier failing step.
 *
 * Deliberately NOT emulating standalone mode: nothing on /staff or
 * /settings branches on it (only the customer install prompt, the
 * code-login ordering and the launch beacon do), and the flag makes every
 * page boot fire `POST /api/metrics/launch`. That beacon is rate-limited
 * per client IP with a 2 s minimum gap, and every CI request shares the
 * "unknown" IP bucket — so back-to-back tests booting the app within 2 s
 * got a 429, which WebKit logs as a console error (CI run 38712421537).
 * The exits under test must work in a plain Safari tab too.
 */

/** iPhone (WebKit), admin signed in, sitting on `path`. */
async function openAsAdmin(page: Page, path: string): Promise<void> {
    await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
    await page.goto(path);
    await expect(page.locator('[data-testid="nav-more"]')).toBeVisible();
}

async function openMoreSheet(page: Page): Promise<void> {
    await page.locator('[data-testid="nav-more"]').tap();
    await expect(page.locator('[data-testid="more-sheet"]')).toBeVisible();
}

async function expectSheetGone(page: Page): Promise<void> {
    await expect(page.locator('[data-testid="more-sheet"]')).toHaveCount(0);
    await expect(page.locator('.sheet-backdrop')).toHaveCount(0);
}

test.describe('iPhone PWA — every overlay and sub-page has a way out (#386)', () => {
    test('Viac -> Nastavenia lands on /settings with the sheet gone and the bottom nav usable', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await openAsAdmin(page, '/staff');

        await openMoreSheet(page);
        await page.locator('[data-testid="more-settings"]').tap();
        await expect(page).toHaveURL(/\/settings$/);
        await expectSheetGone(page);
        await expect(page.locator('[data-testid="nav-desk"]')).toBeVisible();

        assertCleanConsole(consoleMessages);
    });

    test('the More sheet has a visible close button that closes it', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await openAsAdmin(page, '/staff');

        await openMoreSheet(page);
        const close = page.locator('[data-testid="more-sheet"] [data-testid="sheet-close"]');
        await expect(close).toBeVisible();
        await close.tap();
        await expectSheetGone(page);
        await expect(page).toHaveURL(/\/staff$/);

        assertCleanConsole(consoleMessages);
    });

    test('a tap on the backdrop closes the sheet (and the backdrop is marked clickable for iOS)', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await openAsAdmin(page, '/staff');

        await openMoreSheet(page);
        // iOS WebKit only dispatches `click` for a tap on an element it
        // considers clickable; cursor:pointer is what marks a plain <div>.
        await expect(page.locator('.sheet-backdrop')).toHaveCSS('cursor', 'pointer');
        // The dimmed area ABOVE the bottom sheet.
        await page.locator('.sheet-backdrop').tap({ position: { x: 20, y: 20 } });
        await expectSheetGone(page);

        assertCleanConsole(consoleMessages);
    });

    test('/settings has a back control in its header that returns to the desk', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await openAsAdmin(page, '/settings');

        const back = page.locator('[data-testid="settings-back"]');
        await expect(back).toBeVisible();
        await back.tap();
        await expect(page).toHaveURL(/\/staff$/);

        assertCleanConsole(consoleMessages);
    });

    // CI run 38712421537: WebKit never reuses an `as="fetch"` <link
    // rel=preload> for the fetch() wasm-bindgen's init() makes, so the wasm
    // was downloaded TWICE on every iPhone boot and Safari logged "preloaded
    // using link preload but not used". Trunk.toml's `pattern_preload` now
    // preloads only the JS module; this pins the single download.
    test('app boot downloads the wasm bundle exactly once (no unused preload)', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        const wasmRequests: string[] = [];
        page.on('request', (req) => {
            if (new URL(req.url()).pathname.endsWith('.wasm')) wasmRequests.push(req.url());
        });
        await page.goto('/login');
        await expect(page.locator('h1.page-title')).toBeVisible();
        await expect(page.locator('link[rel="preload"][href$=".wasm"]')).toHaveCount(0);
        // Safari reports an unused preload "within a few seconds from the
        // window's load event"; wait past that window so the console check
        // below would actually see such a warning.
        await page.waitForTimeout(4000);
        expect(wasmRequests, 'the wasm must be fetched once, by init() only').toHaveLength(1);

        assertCleanConsole(consoleMessages);
    });

    for (const lang of ['en', 'sk'] as const) {
        test(`/settings services tab never widens the page past the phone screen (${lang})`, async ({ page }) => {
            const consoleMessages = setupConsoleCheck(page);
            // The owner uses Slovak, whose longer tab labels are what
            // overflowed (#386 review) — so measure both languages.
            await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123', lang);
            await page.goto('/settings');
            await expect(page.locator('[data-testid="nav-more"]')).toBeVisible();

            // Prove the requested language really loaded, so the sk run can
            // never silently re-measure English.
            await expect(page.locator('[data-testid="admin-tab-services"]')).toHaveText(
                lang === 'sk' ? 'Sluzby' : 'Services',
            );
            await page.locator('[data-testid="admin-tab-services"]').tap();
            const tableScroll = page.locator('[data-testid="services-table-scroll"]');
            await expect(tableScroll).toBeVisible();
            await expect(tableScroll).toHaveCSS('overflow-x', 'auto');
            const pageOverflow = await page.evaluate(
                () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
            );
            expect(pageOverflow, `the /settings page must not scroll horizontally on a phone (${lang})`).toBeLessThanOrEqual(0);

            assertCleanConsole(consoleMessages);
        });
    }
});
