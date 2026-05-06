import { test, expect, Page } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

async function createUniqueUserLocal(token: string, suffix: string): Promise<string> {
    const cardCode = `LNG-${suffix}`;
    const name = `L Lang${suffix}`;
    const resp = await fetch(`${BASE_URL}/api/users`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({
            name,
            initial_credit: 50,
            card_code: cardCode,
        }),
    });
    if (!resp.ok) throw new Error(`createUniqueUser failed: ${resp.status} ${await resp.text()}`);
    return `Lang${suffix}`;
}

async function openCardByLastName(page: Page, lastName: string) {
    const search = page.locator('input[type="search"]');
    await search.waitFor();
    await search.focus();
    await page.keyboard.type(lastName, { delay: 30 });
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
}

test.describe('Card action form — service dropdown is language-aware', () => {
    test('Refreshments shows in EN and Občerstvenie shows in SK', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const suffix = `${Date.now()}`;
        const lastName = await createUniqueUserLocal(token, suffix);

        // Default in tests is English (loginViaAPI -> setEnglishLanguage).
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        const optionsEn = await page
            .locator('[data-testid="charge-service"] option')
            .allTextContents();
        expect(optionsEn.some(o => o.includes('Refreshments'))).toBe(true);
        expect(optionsEn.some(o => o.includes('Supplements'))).toBe(true);

        // Switch to Slovak. loginViaAPI added an init script forcing 'en' on
        // every page load, so layering a second init script that runs AFTER
        // it ensures 'sk' wins the localStorage write before the WASM boots.
        await page.addInitScript(() => {
            try {
                localStorage.setItem('spinbike_lang', 'sk');
            } catch {
                // ignore — storage not ready
            }
        });
        await page.reload();
        await openCardByLastName(page, lastName);

        const optionsSk = await page
            .locator('[data-testid="charge-service"] option')
            .allTextContents();
        expect(optionsSk.some(o => o.includes('Občerstvenie'))).toBe(true);
        expect(optionsSk.some(o => o.includes('Doplnky výživy'))).toBe(true);

        // The Monthly pass option always carries data-kind regardless of Lang.
        const passOption = page.locator(
            '[data-testid="charge-service"] option[data-kind="monthly_pass"]'
        );
        await expect(passOption).toHaveCount(1);

        assertCleanConsole(msgs);
    });
});
