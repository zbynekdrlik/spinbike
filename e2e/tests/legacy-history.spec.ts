import { test, expect, Page } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

async function seedCardWithBackfilledHistory(token: string, barcode: string): Promise<void> {
    const resp = await fetch(`${BASE_URL}/api/test/seed-transactions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({
            barcode,
            entries: [
                { amount: 1.66, action: 'debit', service_name_sk: 'Občerstvenie' },
                { amount: 3.15, action: 'debit', service_name_sk: 'Doplnky výživy' },
                { amount: 2.5, action: 'debit', service_name_sk: 'Aktivácia karty' },
            ],
        }),
    });
    if (!resp.ok) throw new Error(`seed-transactions failed: ${resp.status} ${await resp.text()}`);
}

async function openCardByBarcode(page: Page, barcode: string) {
    const search = page.locator('input[type="search"]');
    await search.waitFor();
    await search.focus();
    await page.keyboard.type(barcode, { delay: 30 });
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
}

test('card history shows backfilled service categories (English)', async ({ page }) => {
    const msgs = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
    const barcode = `LH-${Date.now()}`;
    await seedCardWithBackfilledHistory(token, barcode);

    await page.goto('/staff');
    await openCardByBarcode(page, barcode);

    // Default Lang in tests is English (set by loginViaAPI).
    // The transactions list renders display_name(lang) — so for English we
    // expect Refreshments / Supplements / Card activation fee.
    const list = page.locator('[data-testid="transactions-list"]');
    await expect(list).toContainText('Refreshments');
    await expect(list).toContainText('Supplements');
    await expect(list).toContainText('Card activation fee');

    assertCleanConsole(msgs);
});

test('card history shows backfilled service categories in Slovak', async ({ page }) => {
    const msgs = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
    const barcode = `LH-SK-${Date.now()}`;
    await seedCardWithBackfilledHistory(token, barcode);

    // Switch to Slovak. loginViaAPI added an init script forcing 'en' on
    // every page load, so layering a second init script that runs AFTER it
    // ensures 'sk' wins the localStorage write before the WASM boots.
    await page.addInitScript(() => {
        try {
            localStorage.setItem('spinbike_lang', 'sk');
        } catch {
            // ignore — storage not ready
        }
    });
    await page.goto('/staff');
    await openCardByBarcode(page, barcode);

    const list = page.locator('[data-testid="transactions-list"]');
    await expect(list).toContainText('Občerstvenie');
    await expect(list).toContainText('Doplnky výživy');
    await expect(list).toContainText('Aktivácia karty');

    assertCleanConsole(msgs);
});
