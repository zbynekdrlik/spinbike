import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

async function openJanaCard(page: import('@playwright/test').Page) {
    await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
    await page.goto('/staff');
    await page.waitForSelector('input[type="search"]');
    await page.fill('input[type="search"]', 'Jana');
    const result = page.locator('[data-testid="search-result"]').first();
    await expect(result).toBeVisible({ timeout: 3000 });
    await result.click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
    // The card detail now defaults to the History tab; the spin-booking tests
    // operate on Upcoming/Persistent so switch over up front.
    await page.locator('[data-testid="tab-upcoming"]').click();
    await expect(page.locator('[data-testid="upcoming-classes"]')).toBeVisible();
}

test.describe('spin booking', () => {
    test('staff books a card for one class', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await openJanaCard(page);

        // Click the first available BOOK button.
        const bookBtn = page.locator('[data-testid^="book-"]').first();
        await expect(bookBtn).toBeVisible({ timeout: 3000 });
        const testId = await bookBtn.getAttribute('data-testid');
        expect(testId).toMatch(/^book-\d+-\d{4}-\d{2}-\d{2}$/);
        await bookBtn.click();

        // The row should now show a red Cancel button.
        await expect(
            page.locator('[data-testid="upcoming-classes"] .btn--danger')
        ).toHaveCount(1, { timeout: 5000 });

        assertCleanConsole(consoleMessages);
    });

    test('staff turns persistent booking ON, AUTO rows appear', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await openJanaCard(page);

        // Switch to the Persistent tab for the toggle interactions.
        await page.locator('[data-testid="tab-persistent"]').click();
        await expect(page.locator('[data-testid="persistent-toggles"]')).toBeVisible();

        // The toggle buttons are populated by an async fetch — wait for the
        // first one before counting, otherwise count() races the fetch.
        const toggles = page.locator('[data-testid^="persistent-toggle-"]');
        await expect(toggles.first()).toBeVisible({ timeout: 10000 });
        const n = await toggles.count();
        expect(n).toBeGreaterThan(0);

        let flipped = false;
        for (let i = 0; i < n; i++) {
            const t = toggles.nth(i);
            const label = (await t.textContent())?.trim();
            if (label === 'On') {
                await t.click();
                await expect(t).toHaveText('Off', { timeout: 5000 });
                flipped = true;
                break;
            }
        }
        expect(flipped).toBe(true);

        // Switch back to Upcoming to verify AUTO rows materialised for the flipped subscription.
        await page.locator('[data-testid="tab-upcoming"]').click();
        await expect(page.locator('[data-testid^="auto-cancel-"]').first()).toBeVisible({
            timeout: 5000,
        });

        assertCleanConsole(consoleMessages);
    });

    test('staff skips one AUTO week, seat returns to BOOK', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await openJanaCard(page); // leaves us on the Upcoming tab

        // Ensure at least one AUTO row exists; if not, flip a persistent toggle ON first.
        // The upcoming-classes fetch is async — give it room before deciding it's empty.
        await page.waitForTimeout(500);
        let autoBtn = page.locator('[data-testid^="auto-cancel-"]').first();
        if ((await autoBtn.count()) === 0) {
            await page.locator('[data-testid="tab-persistent"]').click();
            const toggles = page.locator('[data-testid^="persistent-toggle-"]');
            await expect(toggles.first()).toBeVisible({ timeout: 10000 });
            const n = await toggles.count();
            for (let i = 0; i < n; i++) {
                const t = toggles.nth(i);
                if (((await t.textContent())?.trim()) === 'On') {
                    await t.click();
                    await expect(t).toHaveText('Off', { timeout: 5000 });
                    break;
                }
            }
            await page.locator('[data-testid="tab-upcoming"]').click();
            autoBtn = page.locator('[data-testid^="auto-cancel-"]').first();
            await expect(autoBtn).toBeVisible({ timeout: 10000 });
        }

        const dataId = await autoBtn.getAttribute('data-testid');
        expect(dataId).toMatch(/^auto-cancel-\d+-\d{4}-\d{2}-\d{2}$/);
        const bookId = dataId!.replace('auto-cancel-', 'book-');
        await autoBtn.click();

        await expect(page.locator(`[data-testid="${bookId}"]`)).toBeVisible({
            timeout: 5000,
        });

        assertCleanConsole(consoleMessages);
    });

    test('staff turns persistent OFF, AUTO rows disappear', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);
        await openJanaCard(page);

        // Switch to the Persistent tab for the toggle interactions.
        await page.locator('[data-testid="tab-persistent"]').click();
        await expect(page.locator('[data-testid="persistent-toggles"]')).toBeVisible();

        // Find a toggle currently showing "Off" (i.e. subscription IS active — click to turn OFF).
        // If none, turn one ON first so we have something to turn off.
        const toggles = page.locator('[data-testid^="persistent-toggle-"]');
        await expect(toggles.first()).toBeVisible({ timeout: 10000 });
        const n = await toggles.count();
        let hasOff = false;
        for (let i = 0; i < n; i++) {
            if (((await toggles.nth(i).textContent())?.trim()) === 'Off') {
                hasOff = true;
                break;
            }
        }
        if (!hasOff) {
            // None on — turn the first one On.
            const first = toggles.first();
            await first.click();
            await expect(first).toHaveText('Off', { timeout: 5000 });
        }

        // Now click the Off toggle to turn the subscription OFF (button text returns to "On").
        // Capture the template_id from the toggle we flip so we can scope the
        // auto-cancel assertion below to exactly that template.
        let turnedOff = false;
        let offTid: string | null = null;
        const m = await toggles.count();
        for (let i = 0; i < m; i++) {
            const t = toggles.nth(i);
            if (((await t.textContent())?.trim()) === 'Off') {
                offTid = (await t.getAttribute('data-testid'))?.replace('persistent-toggle-', '') ?? null;
                await t.click();
                await expect(t).toHaveText('On', { timeout: 5000 });
                turnedOff = true;
                break;
            }
        }
        expect(turnedOff).toBe(true);
        expect(offTid).not.toBeNull();

        // Turning a toggle off deterministically zeroes ALL auto-cancel rows for
        // that exact template_id: server-side end_persistent cancels every
        // future/uncharged/persistent-source booking for that (user_id,
        // template_id), and the materialiser only re-creates rows for ACTIVE
        // subscriptions. Scope the assertion to that template_id so it fails if
        // the cancellation didn't actually happen.
        await page.waitForTimeout(500);
        await page.locator('[data-testid="tab-upcoming"]').click();
        const remaining = await page.locator(`[data-testid^="auto-cancel-${offTid}-"]`).count();
        expect(remaining).toBe(0);

        assertCleanConsole(consoleMessages);
    });
});
