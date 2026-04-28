import { test, expect, Page } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

async function activateUniqueCard(
    token: string,
    initialCredit: number,
): Promise<{ barcode: string; lastName: string }> {
    const suffix = `${Date.now()}${Math.random().toString(36).slice(2, 6)}`;
    const barcode = `NP-${suffix}`;
    const lastName = `NoPrePrice${suffix}`;
    const resp = await fetch(`${BASE_URL}/api/cards/activate`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ barcode, initial_credit: initialCredit, first_name: 'NP', last_name: lastName }),
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

test.describe('Staff dashboard — no predefined prices (#17)', () => {
    test('service dropdown labels show only the service name (no euro, no number)', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff?lang=en');
        await openCardByLastName(page, lastName);

        const labels = await page
            .locator('[data-testid="charge-service"] option')
            .allTextContents();

        // Filter out the placeholder "(select service)" option.
        const realLabels = labels.filter((l) => l.trim().length > 0 && !/select service/i.test(l));
        expect(realLabels.length).toBeGreaterThan(0);

        for (const label of realLabels) {
            // No euro symbol.
            expect(label).not.toContain('€');
            // No N.NN numeric price.
            expect(label).not.toMatch(/\d+\.\d{2}/);
            // No parenthesised price annotation like "(5.00 €)".
            expect(label).not.toMatch(/\(.*\)/);
        }

        assertCleanConsole(msgs);
    });

    test('amount input stays empty when staff picks Spinning, Fitness, or Monthly pass', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff?lang=en');
        await openCardByLastName(page, lastName);

        const amountInput = page.locator('[data-testid="charge-amount"]');
        const select = page.locator('[data-testid="charge-service"]');

        // The seed creates Spinning, Fitness, and Monthly pass. Pick each by
        // its option text rather than by index — index is unstable across
        // ordering changes.
        for (const name of ['Spinning', 'Fitness', 'Monthly pass']) {
            const option = select.locator('option').filter({ hasText: name }).first();
            const optValue = await option.getAttribute('value');
            expect(optValue, `option "${name}" missing value`).toBeTruthy();
            await select.selectOption(optValue!);
            await expect(amountInput).toHaveValue('');
        }

        assertCleanConsole(msgs);
    });

    test('submit empty amount surfaces inline error and posts no payment request', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff?lang=en');
        await openCardByLastName(page, lastName);

        // Confirm starting credit before any submit.
        const creditLocator = page.locator('[data-testid="card-credit"]');
        await expect(creditLocator).toContainText('50.00');

        // Pick Spinning (input stays empty post-#17).
        const select = page.locator('[data-testid="charge-service"]');
        const spinningOption = select.locator('option').filter({ hasText: 'Spinning' }).first();
        const spinningValue = await spinningOption.getAttribute('value');
        await select.selectOption(spinningValue!);
        await expect(page.locator('[data-testid="charge-amount"]')).toHaveValue('');

        // Track any payment POST that fires during the next 1s. We expect zero.
        let paymentRequestFired = false;
        const offRequest = (req: import('@playwright/test').Request) => {
            if (
                /\/api\/payments\/(charge|sell-pass)/.test(req.url())
                && req.method() === 'POST'
            ) {
                paymentRequestFired = true;
            }
        };
        page.on('request', offRequest);

        await page.locator('[data-testid="charge-submit"]').click();

        // Inline error appears.
        await expect(
            page.locator('[data-testid="action-panel"] .alert-error'),
        ).toBeVisible();

        // Card credit unchanged.
        await expect(creditLocator).toContainText('50.00');

        // Give the page 500ms to fire any async POST. None should.
        await page.waitForTimeout(500);
        page.off('request', offRequest);
        expect(paymentRequestFired).toBe(false);

        assertCleanConsole(msgs);
    });

    test('typed amount still works end-to-end (charge debits the typed value)', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff?lang=en');
        await openCardByLastName(page, lastName);

        const select = page.locator('[data-testid="charge-service"]');
        const spinningOption = select.locator('option').filter({ hasText: 'Spinning' }).first();
        const spinningValue = await spinningOption.getAttribute('value');
        await select.selectOption(spinningValue!);

        // Staff types the price.
        await page.locator('[data-testid="charge-amount"]').fill('7.50');

        const chargeResp = page.waitForResponse(
            (r) => r.url().includes('/api/payments/charge') && r.request().method() === 'POST',
        );
        await page.locator('[data-testid="charge-submit"]').click();
        const resp = await chargeResp;
        expect(resp.ok()).toBe(true);

        // Card credit dropped by 7.50.
        await expect(page.locator('[data-testid="card-credit"]')).toContainText('42.50');

        assertCleanConsole(msgs);
    });
});
