import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

// After the V12 backfill migration, a legacy card's history must classify
// rows by EventKind correctly. We seed a card with one row of each post-
// backfill action shape and assert the per-card transactions list renders
// each EventKind's English label.
test('per-card history renders charge / topup / visit / pass-sale labels', async ({ page }) => {
    const msgs = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
    const barcode = `PBH-${Date.now()}`;

    const seed = await fetch(`${BASE_URL}/api/test/seed-transactions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({
            barcode,
            entries: [
                // TopUp: positive amount, action=topup
                { amount: 10.0, action: 'topup', service_name_sk: 'Občerstvenie' },
                // Charge: negative amount, action=charge, no valid_until
                { amount: -3.0, action: 'charge', service_name_sk: 'Spinning' },
                // Visit: zero amount, action=visit
                { amount: 0.0, action: 'visit', service_name_sk: 'Spinning' },
                // PassSale: any amount with valid_until set wins precedence
                { amount: -35.0, action: 'charge', service_name_sk: 'Mesačná permanentka', valid_until: '2099-12-31' },
            ],
        }),
    });
    if (!seed.ok) throw new Error(`seed failed: ${seed.status} ${await seed.text()}`);

    // Open the card via search.
    await page.goto('/staff');
    const search = page.locator('input[type="search"]');
    await search.waitFor();
    await search.focus();
    await page.keyboard.type(barcode, { delay: 30 });
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

    // English labels (loginViaAPI forces English):
    //   EventKind::TopUp    → "Top-up"
    //   EventKind::Charge   → "Spent from credit"
    //   EventKind::Visit    → "Entry with pass"
    //   EventKind::PassSale → "Sale of pass"
    const list = page.locator('[data-testid="transactions-list"]');
    await expect(list).toBeVisible();
    await expect(list).toContainText('Top-up');
    await expect(list).toContainText('Spent from credit');
    await expect(list).toContainText('Entry with pass');
    await expect(list).toContainText('Sale of pass');

    assertCleanConsole(msgs);
});
