import { test, expect, Page } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

async function createUniqueUser(
    token: string,
    initialCredit: number,
): Promise<{ card_code: string; lastName: string }> {
    const suffix = `${Date.now()}${Math.random().toString(36).slice(2, 6)}`;
    const cardCode = `BL-${suffix}`;
    const lastName = `Btnlayout${suffix}`;
    const resp = await fetch(`${BASE_URL}/api/users`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ name: `BL ${lastName}`, initial_credit: initialCredit, card_code: cardCode }),
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

async function sellMonthlyPass(page: Page) {
    const mpOption = page
        .locator('[data-testid="charge-service"] option')
        .filter({ hasText: /Monthly pass|Mesačná permanentka/ })
        .first();
    await expect(mpOption).toBeAttached();
    const mpValue = await mpOption.getAttribute('value');
    if (!mpValue) throw new Error('Monthly pass option had no value');
    await page.locator('[data-testid="charge-service"]').selectOption(mpValue);
    // Post-#17: amount input stays empty after selectOption — staff types
    // every time. 35.00 matches the seed's monthly_pass default_price so
    // the resulting pass-banner-active assertion downstream still holds.
    await page.locator('[data-testid="charge-amount"]').fill('35.00');
    const sellPassResp = page.waitForResponse(
        (r) => r.url().includes('/api/payments/sell-pass') && r.request().method() === 'POST',
    );
    await page.locator('[data-testid="charge-submit"]').click();
    const resp = await sellPassResp;
    expect(resp.ok()).toBe(true);
    await expect(page.locator('[data-testid="pass-banner-active"]')).toBeVisible();
}

test.describe('Staff dashboard — button layout & colors (#13)', () => {
    test('action-row: Charge left of Topup, with same-hue soft sibling', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await createUniqueUser(token, 80.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        const charge = page.locator('[data-testid="charge-submit"]');
        const topup = page.locator('[data-testid="topup-submit"]');
        await expect(charge).toBeVisible();
        await expect(topup).toBeVisible();

        // Charge precedes Topup in DOM order.
        const topupHandle = await topup.elementHandle();
        const order = await charge.evaluate((c, t) => {
            // Node.DOCUMENT_POSITION_FOLLOWING (4) means the argument follows `this`.
            return (c.compareDocumentPosition(t as Node) & 4) === 4;
        }, topupHandle);
        expect(order).toBe(true);

        // Charge: solid green primary (eye-catching, most-used action).
        // Topup: soft green sibling (same hue, lower saturation — visible but
        // recedes). The earlier `.btn--ghost` rendered nearly invisible against
        // the surface, so the CEO asked for a "small difference" instead.
        await expect(charge).toHaveClass(/\bbtn--primary\b/);
        await expect(charge).not.toHaveClass(/\bbtn--primary-soft\b/);
        await expect(topup).toHaveClass(/\bbtn--primary-soft\b/);
        await expect(topup).not.toHaveClass(/\bbtn--ghost\b/);

        assertCleanConsole(msgs);
    });

    test('visit-row: Fitness left of Spinning with same-hue soft sibling', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await createUniqueUser(token, 80.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);
        await sellMonthlyPass(page);

        const visits = page.locator('[data-testid="log-visit-btn"]');
        await expect(visits).toHaveCount(2);

        // Migrations seed name_en values "Spinning" and "Fitness". The UI must
        // sort by name_en alphabetically: Fitness first, Spinning second.
        const labels = await visits.allTextContents();
        expect(labels[0]).toMatch(/Fitness/);
        expect(labels[1]).toMatch(/Spinning/);

        // Fitness is the more-used activity → solid blue (eye-catching).
        // Spinning is the soft-blue sibling — same hue, lower saturation —
        // so the row reads primary / secondary within one color family.
        await expect(visits.nth(0)).toHaveClass(/\bbtn--info\b/);
        await expect(visits.nth(0)).not.toHaveClass(/\bbtn--info-soft\b/);
        await expect(visits.nth(1)).toHaveClass(/\bbtn--info-soft\b/);
        await expect(visits.nth(1)).not.toHaveClass(/\bbtn--pass\b/);

        assertCleanConsole(msgs);
    });
});
