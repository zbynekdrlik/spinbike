import { test, expect, Page } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

async function activateUniqueCard(
    token: string,
    initialCredit: number,
): Promise<{ barcode: string; lastName: string }> {
    const suffix = `${Date.now()}${Math.random().toString(36).slice(2, 6)}`;
    const barcode = `MP-${suffix}`;
    const lastName = `Monthlypass${suffix}`;
    const resp = await fetch(`${BASE_URL}/api/cards/activate`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ barcode, initial_credit: initialCredit, first_name: 'MP', last_name: lastName }),
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

test.describe('Monthly pass — sell via dropdown, banner, log-visit', () => {
    test('sell pass via dropdown → banner appears → log-visit logs 0 EUR row', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        // Start with 80 € so the 35 € pass charge leaves 45 €.
        const { lastName } = await activateUniqueCard(token, 80.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);
        await expect(page.locator('[data-testid="card-credit"]')).toContainText('80.00');

        // Select Monthly pass from the unified service dropdown.
        const mpValue = await page
            .locator('[data-testid="charge-service"] option')
            .filter({ hasText: 'Monthly pass' })
            .first()
            .getAttribute('value');
        if (!mpValue) throw new Error('Monthly pass option not found');
        await page.locator('[data-testid="charge-service"]').selectOption(mpValue);
        // Post-#17: staff types the price every time. 35.00 keeps the
        // downstream `80 - 35 = 45` card-credit assertion intact.
        await page.locator('[data-testid="charge-amount"]').fill('35.00');
        await page.locator('[data-testid="charge-submit"]').click();

        // Active pass banner appears.
        await expect(page.locator('[data-testid="pass-banner-active"]')).toBeVisible();
        // Credit drops 80 - 35 = 45.
        await expect(page.locator('[data-testid="card-credit"]')).toContainText('45.00');

        // Quick log-visit chip is visible only when an active pass exists.
        const visitBtn = page.locator('[data-testid="log-visit-btn"]').first();
        await expect(visitBtn).toBeVisible();
        await visitBtn.click();

        // Switch to History tab and verify a 0 € visit row appears.
        await page.locator('[data-testid="tab-history"]').click();
        // History rows show transaction amounts; a logged visit is 0,00 € or 0.00 €
        // depending on locale formatting. Match either.
        await expect(page.locator('[data-testid="action-panel"]')).toContainText(/0[.,]00/);

        assertCleanConsole(msgs);
    });
});
