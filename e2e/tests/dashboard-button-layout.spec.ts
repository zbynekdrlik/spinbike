import { test, expect, Page } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

async function activateUniqueCard(
    token: string,
    initialCredit: number,
): Promise<{ barcode: string; lastName: string }> {
    const suffix = `${Date.now()}${Math.random().toString(36).slice(2, 6)}`;
    const barcode = `BL-${suffix}`;
    const lastName = `Btnlayout${suffix}`;
    const resp = await fetch(`${BASE_URL}/api/cards/activate`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ barcode, initial_credit: initialCredit, first_name: 'BL', last_name: lastName }),
    });
    if (!resp.ok) throw new Error(`activate failed: ${resp.status} ${await resp.text()}`);
    return { barcode, lastName };
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
        .filter({ hasText: /Monthly pass|Mesačný preplatok/ })
        .first();
    await expect(mpOption).toBeAttached();
    const mpValue = await mpOption.getAttribute('value');
    if (!mpValue) throw new Error('Monthly pass option had no value');
    await page.locator('[data-testid="charge-service"]').selectOption(mpValue);
    await expect(page.locator('[data-testid="charge-amount"]')).not.toHaveValue('');
    const sellPassResp = page.waitForResponse(
        (r) => r.url().includes('/api/payments/sell-pass') && r.request().method() === 'POST',
    );
    await page.locator('[data-testid="charge-submit"]').click();
    const resp = await sellPassResp;
    expect(resp.ok()).toBe(true);
    await expect(page.locator('[data-testid="pass-banner-active"]')).toBeVisible();
}

test.describe('Staff dashboard — button layout & colors (#13)', () => {
    test('action-row: Charge left of Topup, Topup is ghost-styled', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 80.0);
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

        await expect(charge).toHaveClass(/\bbtn--primary\b/);
        await expect(topup).toHaveClass(/\bbtn--ghost\b/);
        await expect(topup).not.toHaveClass(/\bbtn--primary\b/);

        assertCleanConsole(msgs);
    });

    test('visit-row: Fitness left of Spinning with distinct colors', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 80.0);
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

        await expect(visits.nth(0)).toHaveClass(/\bbtn--info\b/);
        await expect(visits.nth(1)).toHaveClass(/\bbtn--pass\b/);

        assertCleanConsole(msgs);
    });
});
