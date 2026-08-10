import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI, uniqueLetterSuffix } from './helpers';

/**
 * #319: the customer-facing top header collapses to ONE row —
 * `[logo] [shortened name] [burger]` — with the destination links
 * (My Bookings / Balance / Settings), Logout, and the language toggle
 * moved into a burger-triggered `Sheet` (same component `AdaptiveNav`
 * already uses for staff's "More" sheet). The standalone "Ahoj, <name>"
 * greeting on /my/balance is removed entirely — the shortened name in the
 * header already confirms "logged in as the right account".
 *
 * Staff/admin are UNTOUCHED by this ticket (their `.navbar-links` is
 * already fully hidden by the pre-existing
 * `body:has(.adaptive-nav) .navbar-links { display: none; }` rule) — see
 * `nav-adaptive.spec.ts` for that coverage, unchanged here.
 */

const BASE_URL = 'http://localhost:8099';

async function seedNamedCustomer(
    baseURL: string,
    name: string,
): Promise<{ email: string; password: string }> {
    const suffix = uniqueLetterSuffix();
    const email = `nav-burger-${suffix}@test.local`;
    const password = `Pw-${suffix}`;
    const resp = await fetch(`${baseURL}/api/test/seed-account`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email, password, name, role: 'customer' }),
    });
    if (!resp.ok) {
        throw new Error(`seed-account failed: ${resp.status} ${await resp.text()}`);
    }
    return { email, password };
}

test.describe('Customer header burger menu (#319)', () => {
    test('header is a single row (logo + shortened name + burger) — no raw text links', async ({
        page,
    }) => {
        const messages = setupConsoleCheck(page);
        const { email, password } = await seedNamedCustomer(BASE_URL, 'Zbynek Testovaci');
        await page.setViewportSize({ width: 390, height: 844 });
        await loginViaAPI(page, BASE_URL, email, password);
        await page.goto('/my/balance');
        await page.waitForSelector('[data-testid="navbar-burger"]');

        await expect(page.locator('.navbar-brand')).toBeVisible();
        await expect(page.locator('[data-testid="navbar-user-name"]')).toHaveText('Zbynek T.');
        await expect(page.locator('[data-testid="navbar-burger"]')).toBeVisible();

        // The destination links / logout / lang-toggle no longer render
        // directly in the header — they live behind the (closed) burger.
        await expect(page.locator('.navbar-links a[href="/my/bookings"]')).toHaveCount(0);
        await expect(page.locator('.navbar-links a[href="/my/settings"]')).toHaveCount(0);
        await expect(page.locator('[data-testid="menu-logout"]')).toHaveCount(0);

        // Materially shorter than the pre-#319 145px 3-row header.
        const navHeight = await page
            .locator('.navbar')
            .evaluate((el) => el.getBoundingClientRect().height);
        expect(navHeight).toBeLessThan(80);

        assertCleanConsole(messages);
    });

    test('the standalone "Ahoj, ..." greeting no longer appears on /my/balance', async ({ page }) => {
        const messages = setupConsoleCheck(page);
        const { email, password } = await seedNamedCustomer(BASE_URL, 'Greeting Removed');
        await loginViaAPI(page, BASE_URL, email, password);
        await page.goto('/my/balance');
        await page.waitForSelector('h1.page-title');

        // loginViaAPI defaults the locale to English (see e2e-testing
        // skill's helpers.ts gotcha) — the static, non-personalized title
        // every other page already uses.
        const title = await page.textContent('h1.page-title');
        expect(title).toBe('My Balance');
        expect(title).not.toContain('Greeting Removed');
        expect(title?.toLowerCase()).not.toContain('ahoj');

        assertCleanConsole(messages);
    });

    test('burger toggles aria-expanded/aria-controls, reveals all 5 items, and navigates', async ({
        page,
    }) => {
        const messages = setupConsoleCheck(page);
        const { email, password } = await seedNamedCustomer(BASE_URL, 'Burger Opener');
        await loginViaAPI(page, BASE_URL, email, password);
        await page.goto('/my/balance');

        const burger = page.locator('[data-testid="navbar-burger"]');
        await expect(burger).toHaveAttribute('aria-expanded', 'false');
        const controlsId = await burger.getAttribute('aria-controls');
        expect(controlsId).toBeTruthy();

        await burger.click();
        await expect(burger).toHaveAttribute('aria-expanded', 'true');

        const sheet = page.locator('[data-testid="navbar-menu-sheet"]');
        await expect(sheet).toBeVisible();
        // aria-controls genuinely points at the opened sheet's own id.
        await expect(page.locator(`#${controlsId}`)).toHaveCount(1);

        await expect(sheet.locator('[data-testid="menu-my-bookings"]')).toBeVisible();
        await expect(sheet.locator('[data-testid="menu-balance"]')).toBeVisible();
        await expect(sheet.locator('[data-testid="menu-settings"]')).toBeVisible();
        await expect(sheet.locator('[data-testid="menu-logout"]')).toBeVisible();
        await expect(sheet.locator('[data-testid="menu-lang-toggle"]')).toBeVisible();

        await sheet.locator('[data-testid="menu-my-bookings"]').click();
        await page.waitForURL('**/my/bookings');

        assertCleanConsole(messages);
    });

    test('burger closes on Escape and restores focus to the toggle button', async ({ page }) => {
        const messages = setupConsoleCheck(page);
        const { email, password } = await seedNamedCustomer(BASE_URL, 'Escape Closer');
        await loginViaAPI(page, BASE_URL, email, password);
        await page.goto('/my/balance');

        const burger = page.locator('[data-testid="navbar-burger"]');
        await burger.click();
        const sheet = page.locator('[data-testid="navbar-menu-sheet"]');
        await expect(sheet).toBeVisible();

        // No prior click landed inside the sheet — proving focus was moved
        // into it programmatically on open, not left on the trigger (a
        // dialog must not depend on a mouse click to become keyboard-
        // dismissable, see #319's a11y requirement).
        await page.keyboard.press('Escape');
        await expect(sheet).toBeHidden();
        await expect(burger).toHaveAttribute('aria-expanded', 'false');
        await expect(burger).toBeFocused();

        assertCleanConsole(messages);
    });

    test('burger closes on an outside (backdrop) click', async ({ page }) => {
        const messages = setupConsoleCheck(page);
        const { email, password } = await seedNamedCustomer(BASE_URL, 'Outside Clicker');
        await loginViaAPI(page, BASE_URL, email, password);
        await page.goto('/my/balance');

        await page.locator('[data-testid="navbar-burger"]').click();
        const sheet = page.locator('[data-testid="navbar-menu-sheet"]');
        await expect(sheet).toBeVisible();

        // Click the backdrop far from the sheet panel itself.
        await page.locator('.sheet-backdrop').click({ position: { x: 5, y: 5 } });
        await expect(sheet).toBeHidden();

        assertCleanConsole(messages);
    });
});
