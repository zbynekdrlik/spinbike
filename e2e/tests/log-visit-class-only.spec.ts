import { test, expect, Page } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

async function createUniqueUser(
    token: string,
    initialCredit: number,
): Promise<{ card_code: string; lastName: string }> {
    const suffix = `${Date.now()}${Math.random().toString(36).slice(2, 6)}`;
    const cardCode = `LV-${suffix}`;
    const lastName = `Logvisit${suffix}`;
    const resp = await fetch(`${BASE_URL}/api/users`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ name: `LV ${lastName}`, initial_credit: initialCredit, card_code: cardCode }),
    });
    if (!resp.ok) throw new Error(`createUniqueUser failed: ${resp.status} ${await resp.text()}`);
    return { card_code: cardCode, lastName };
}

async function openCardByLastName(page: Page, lastName: string) {
    const searchInput = page.locator('input[type="search"]');
    await searchInput.waitFor();
    await searchInput.focus();
    await page.keyboard.type(lastName, { delay: 30 });
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
}

test.describe('Log Visit — only class-visit services', () => {
    test('with active pass, only Spinning and Fitness appear as Log Visit buttons', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await createUniqueUser(token, 80.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        // Sell a monthly pass so log-visit chips become visible. The dropdown
        // populates async after services load — wait for the option to exist,
        // then explicitly type the price (post-#17: no auto-fill, staff types
        // every time, otherwise parse_money returns early and no pass is sold).
        const mpOption = page
            .locator('[data-testid="charge-service"] option')
            .filter({ hasText: /Monthly pass|Mesačná permanentka/ })
            .first();
        await expect(mpOption).toBeAttached();
        const mpValue = await mpOption.getAttribute('value');
        if (!mpValue) throw new Error('Monthly pass option had no value');
        await page.locator('[data-testid="charge-service"]').selectOption(mpValue);
        await page.locator('[data-testid="charge-amount"]').fill('35.00');

        const sellPassResp = page.waitForResponse(
            (r) => r.url().includes('/api/payments/sell-pass') && r.request().method() === 'POST',
        );
        await page.locator('[data-testid="charge-submit"]').click();
        const resp = await sellPassResp;
        expect(resp.ok()).toBe(true);
        await expect(page.locator('[data-testid="pass-banner-active"]')).toBeVisible();

        // Migrations seed 5 generic services (Spinning, Fitness, Refreshments,
        // Supplements, Card activation fee). Only the two class-visit services
        // should produce Log Visit buttons.
        const visitButtons = page.locator('[data-testid="log-visit-btn"]');
        await expect(visitButtons).toHaveCount(2);

        const labels = await visitButtons.allTextContents();
        const joined = labels.join(' | ');
        expect(joined).toMatch(/Spinning/);
        expect(joined).toMatch(/Fitness/);
        expect(joined).not.toMatch(/Refreshments|Občerstvenie/);
        expect(joined).not.toMatch(/Supplements|Doplnky/);
        expect(joined).not.toMatch(/Card activation|Aktiv[aá]cia/);

        // Items remain available in the charge dropdown — they're sellable, just
        // not visit-loggable.
        const chargeOptions = await page
            .locator('[data-testid="charge-service"] option')
            .allTextContents();
        const chargeJoined = chargeOptions.join(' | ');
        expect(chargeJoined).toMatch(/Refreshments|Občerstvenie/);
        expect(chargeJoined).toMatch(/Supplements|Doplnky/);

        assertCleanConsole(msgs);
    });
});
