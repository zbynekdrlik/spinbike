import { test, expect, devices } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

// Manifest / PNG-icon eligibility — the static, browser-agnostic surface
// Chromium's install-prompt heuristic actually checks. No page/browser
// needed; plain HTTP requests via Playwright's `request` fixture.
test.describe('Install-to-home-screen manifest eligibility (#110)', () => {
    test('manifest.json lists PNG icon entries (any + maskable) and each PNG resolves 200', async ({ request }) => {
        const manifestResp = await request.get(`${BASE_URL}/manifest.json`);
        expect(manifestResp.ok()).toBe(true);
        const manifest = await manifestResp.json();

        const icons = manifest.icons as Array<{ src: string; sizes: string; type: string; purpose?: string }>;
        const pngIcons = icons.filter((i) => i.type === 'image/png');
        expect(pngIcons.length).toBeGreaterThanOrEqual(4);

        const anySizes = pngIcons.filter((i) => i.purpose === 'any').map((i) => i.sizes).sort();
        const maskableSizes = pngIcons.filter((i) => i.purpose === 'maskable').map((i) => i.sizes).sort();
        expect(anySizes).toEqual(['192x192', '512x512']);
        expect(maskableSizes).toEqual(['192x192', '512x512']);

        // The original SVG entry must survive — kept per the design map.
        expect(icons.some((i) => i.type === 'image/svg+xml')).toBe(true);

        for (const icon of pngIcons) {
            const resp = await request.get(`${BASE_URL}${icon.src}`);
            expect(resp.ok(), `${icon.src} should resolve 200`).toBe(true);
            expect(resp.headers()['content-type']).toContain('image/png');
        }
    });
});

// iOS Safari: no `beforeinstallprompt` event exists there at all, so the
// component must fall back to the UA-sniffed 2-step Share guide. Emulated by
// applying the iPhone 13 device descriptor's context options (userAgent,
// viewport, isMobile, hasTouch) WITHOUT its `defaultBrowserType: 'webkit'` —
// spreading the whole descriptor forces a new worker mid-describe-block
// (Playwright only allows `defaultBrowserType` at file/config top level), and
// we don't need real WebKit here — only a userAgent containing "iPhone",
// which is exactly what the component's UA sniff checks.
const iPhone = devices['iPhone 13'];
test.describe('Install-to-home-screen component — iOS Safari guide', () => {
    test.use({
        userAgent: iPhone.userAgent,
        viewport: iPhone.viewport,
        isMobile: iPhone.isMobile,
        hasTouch: iPhone.hasTouch,
    });

    test('renders the 2-step Share -> Add to Home Screen guide on /my/balance', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'customer@test.com', 'password123');
        await page.goto('/my/balance');
        await page.waitForSelector('[data-testid="door-open-button"]', { timeout: 10000 });

        await expect(page.locator('[data-testid="install-prompt-ios"]')).toBeVisible();
        await expect(page.locator('[data-testid="install-prompt-ios-step1"]')).toBeVisible();
        await expect(page.locator('[data-testid="install-prompt-ios-step2"]')).toBeVisible();
        // The Android/Chromium button must never render on iOS.
        await expect(page.locator('[data-testid="install-prompt-android"]')).toHaveCount(0);

        assertCleanConsole(consoleMessages);
    });
});

// Real iPads since iPadOS 13 default to "Request Desktop Website", so
// navigator.userAgent reports as a plain Mac with NO "iPad" substring at
// all — a bare UA-substring check misses every stock-configured iPad. The
// component disambiguates via navigator.platform === "MacIntel" combined
// with navigator.maxTouchPoints > 1 (a real Mac has none). Emulated here
// with a genuine desktop-Safari-on-Mac userAgent plus a JS override of
// platform/maxTouchPoints, since Playwright's device descriptors don't
// model this iPadOS-specific quirk.
test.describe('Install-to-home-screen component — iPadOS (desktop-spoofed UA)', () => {
    test.use({
        userAgent:
            'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_6) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15',
        viewport: { width: 1024, height: 1366 },
    });

    test('renders the iOS guide via the maxTouchPoints disambiguator', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await page.addInitScript(() => {
            Object.defineProperty(window.navigator, 'platform', { get: () => 'MacIntel' });
            Object.defineProperty(window.navigator, 'maxTouchPoints', { get: () => 5 });
        });
        await loginViaAPI(page, BASE_URL, 'customer@test.com', 'password123');
        await page.goto('/my/balance');
        await page.waitForSelector('[data-testid="door-open-button"]', { timeout: 10000 });

        await expect(page.locator('[data-testid="install-prompt-ios"]')).toBeVisible();
        await expect(page.locator('[data-testid="install-prompt-android"]')).toHaveCount(0);

        assertCleanConsole(consoleMessages);
    });

    test('a real Mac (maxTouchPoints = 0) shows neither install surface', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await page.addInitScript(() => {
            Object.defineProperty(window.navigator, 'platform', { get: () => 'MacIntel' });
            Object.defineProperty(window.navigator, 'maxTouchPoints', { get: () => 0 });
        });
        await loginViaAPI(page, BASE_URL, 'customer@test.com', 'password123');
        await page.goto('/my/balance');
        await page.waitForSelector('[data-testid="door-open-button"]', { timeout: 10000 });

        await expect(page.locator('[data-testid="install-prompt-ios"]')).toHaveCount(0);
        await expect(page.locator('[data-testid="install-prompt-android"]')).toHaveCount(0);

        assertCleanConsole(consoleMessages);
    });
});

// Desktop Chromium (the default project browser, no device override): no
// beforeinstallprompt is fired by a normal desktop tab and the UA isn't
// iPhone/iPad, so neither install surface should render.
test.describe('Install-to-home-screen component — desktop Chromium', () => {
    test('neither the iOS guide nor the Android button renders without a captured event', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'customer@test.com', 'password123');
        await page.goto('/my/balance');
        await page.waitForSelector('[data-testid="door-open-button"]', { timeout: 10000 });

        await expect(page.locator('[data-testid="install-prompt-ios"]')).toHaveCount(0);
        await expect(page.locator('[data-testid="install-prompt-android"]')).toHaveCount(0);

        assertCleanConsole(consoleMessages);
    });

    test('a simulated beforeinstallprompt event renders the Android button and .prompt() fires on click', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'customer@test.com', 'password123');

        // The native install dialog itself can't fire headless. Instead we
        // simulate the browser firing `beforeinstallprompt` the same way a
        // real eligible Chromium install would: it fires early, well before
        // our WASM bundle loads, so index.html's own listener (the thing
        // under test's real capture path) picks it up into
        // window.__deferredInstallPrompt exactly like production. We attach
        // a mock `.prompt()` / `.userChoice` so clicking our button proves
        // the component actually replays the captured event.
        await page.addInitScript(() => {
            (window as unknown as { __installPromptCalls: number }).__installPromptCalls = 0;
            window.addEventListener('DOMContentLoaded', () => {
                const fakeEvent = new Event('beforeinstallprompt') as Event & {
                    prompt: () => Promise<void>;
                    userChoice: Promise<{ outcome: string }>;
                };
                fakeEvent.prompt = () => {
                    (window as unknown as { __installPromptCalls: number }).__installPromptCalls += 1;
                    return Promise.resolve();
                };
                fakeEvent.userChoice = Promise.resolve({ outcome: 'accepted' });
                window.dispatchEvent(fakeEvent);
            });
        });

        await page.goto('/my/balance');
        await page.waitForSelector('[data-testid="door-open-button"]', { timeout: 10000 });

        const button = page.locator('[data-testid="install-prompt-button"]');
        await expect(button).toBeVisible();
        await button.click();

        // Hides immediately on click — the captured event is single-use.
        await expect(button).toHaveCount(0);

        const calls = await page.evaluate(
            () => (window as unknown as { __installPromptCalls: number }).__installPromptCalls,
        );
        expect(calls).toBe(1);

        assertCleanConsole(consoleMessages);
    });
});
