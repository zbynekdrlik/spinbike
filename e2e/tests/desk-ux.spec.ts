import { test, expect, Page } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

async function activateUniqueCard(
    token: string,
    initialCredit: number,
): Promise<{ barcode: string; lastName: string }> {
    const suffix = `${Date.now()}${Math.random().toString(36).slice(2, 6)}`;
    const barcode = `UX-${suffix}`;
    const lastName = `Ux${suffix}`;
    const resp = await fetch(`${BASE_URL}/api/cards/activate`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ barcode, initial_credit: initialCredit, first_name: 'UX', last_name: lastName }),
    });
    if (!resp.ok) throw new Error(`activate failed: ${resp.status} ${await resp.text()}`);
    return { barcode, lastName };
}

async function sellPassToCard(
    token: string,
    cardId: number,
    daysFromToday: number,
): Promise<void> {
    const validUntil = new Date(Date.now() + daysFromToday * 86400e3)
        .toISOString()
        .slice(0, 10);
    const resp = await fetch(`${BASE_URL}/api/payments/sell-pass`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ card_id: cardId, price: 35.0, valid_until: validUntil }),
    });
    if (!resp.ok) throw new Error(`sell-pass failed: ${resp.status} ${await resp.text()}`);
}

async function lookupCardId(token: string, barcode: string): Promise<number> {
    const resp = await fetch(
        `${BASE_URL}/api/cards/lookup/${encodeURIComponent(barcode)}`,
        { headers: { Authorization: `Bearer ${token}` } },
    );
    if (!resp.ok) throw new Error(`lookup failed: ${resp.status}`);
    const body = await resp.json();
    return body.id as number;
}

async function openCardByLastName(page: Page, lastName: string) {
    const searchInput = page.locator('input[type="search"]');
    await searchInput.waitFor();
    await searchInput.focus();
    await page.keyboard.type(lastName, { delay: 30 });
    await page.locator('[data-testid="search-result"]').first().click();
    await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();
}

test.describe('Staff desk UX cluster — issues #29 #30 #31 #32', () => {
    test('fitness preselected on form open', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        const select = page.locator('[data-testid="charge-service"]');
        const value = await select.inputValue();
        expect(value).not.toBe('');

        const fitnessOption = select.locator('option', { hasText: /Fitness/ });
        const fitnessValue = await fitnessOption.first().getAttribute('value');
        expect(value).toBe(fitnessValue);

        await assertCleanConsole(msgs);
    });

    test('charge form has no empty service option', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        // Empty-value option should be gone — placeholder removed in #29.
        const emptyOption = page.locator('[data-testid="charge-service"] option[value=""]');
        await expect(emptyOption).toHaveCount(0);

        await assertCleanConsole(msgs);
    });

    test('quick spinning charge button charges card', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        const quick = page.locator('[data-testid="quick-charge-spinning"]');
        await expect(quick).toBeVisible();
        // Label format: "Spinning {price} €"
        await expect(quick).toHaveText(/^Spinning \d+\.\d{2} €$/);

        const chargeResp = page.waitForResponse(
            (r) => r.url().includes('/api/payments/charge') && r.request().method() === 'POST',
        );
        await quick.click();
        const resp = await chargeResp;
        expect(resp.ok()).toBe(true);

        // Verify the new transaction appears in the card history.
        await expect(
            page.locator('[data-testid="txn-row"]').first(),
        ).toBeVisible();

        await assertCleanConsole(msgs);
    });

    test('card header shows name and barcode on one line', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName, barcode } = await activateUniqueCard(token, 50.0);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        const title = page.locator('[data-testid="action-panel"] .card-title');
        await expect(title).toBeVisible();
        await expect(title).toContainText(lastName);
        await expect(title).toContainText(barcode);

        // .card-header__meta div is gone after #32b — barcode lives inside .card-title.
        const meta = page.locator('[data-testid="action-panel"] .card-header__meta');
        await expect(meta).toHaveCount(0);

        // Name font-size visibly larger (≥ 24px).
        const nameFontSize = await page
            .locator('.card-title__name')
            .first()
            .evaluate((el) => parseFloat(getComputedStyle(el).fontSize));
        expect(nameFontSize).toBeGreaterThanOrEqual(24);

        await assertCleanConsole(msgs);
    });

    test('pass banner active is one line', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName, barcode } = await activateUniqueCard(token, 100.0);
        const cardId = await lookupCardId(token, barcode);
        await sellPassToCard(token, cardId, 14);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        const banner = page.locator('[data-testid="pass-banner-active"]');
        await expect(banner).toBeVisible();
        await expect(banner).toHaveText(/^✓ Mesačný lístok do \d{1,2}\.\d{1,2}\.\d{4} \(\d+ dní\)/u);

        // Pencil edit button is present and inside the same single-line container.
        const editBtn = banner.locator('[data-testid="pass-date-edit"]');
        await expect(editBtn).toBeVisible();
        // No legacy `.pass-banner-sub` div.
        await expect(banner.locator('.pass-banner-sub')).toHaveCount(0);

        await assertCleanConsole(msgs);
    });

    test('pass banner expired is one line', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName, barcode } = await activateUniqueCard(token, 100.0);
        const cardId = await lookupCardId(token, barcode);
        await sellPassToCard(token, cardId, -5); // expired 5 days ago
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        const banner = page.locator('[data-testid="pass-banner-expired"]');
        await expect(banner).toBeVisible();
        // Symmetric guard: expired must also be a single line, no .pass-banner-sub.
        await expect(banner.locator('.pass-banner-sub')).toHaveCount(0);

        await assertCleanConsole(msgs);
    });

    test('Cards — Quick Dashboard h1 is gone', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff');
        await expect(page.locator('h1.page-title')).toHaveCount(0);
        const body = (await page.locator('body').textContent()) ?? '';
        expect(body.toLowerCase()).not.toContain('cards — quick dashboard');
        expect(body.toLowerCase()).not.toContain('karty — rychly prehlad');

        await assertCleanConsole(msgs);
    });

    test('log-visit class buttons are bigger and bolder', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName, barcode } = await activateUniqueCard(token, 100.0);
        const cardId = await lookupCardId(token, barcode);
        await sellPassToCard(token, cardId, 14);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        const visitBtn = page.locator('[data-testid="log-visit-btn"]').first();
        await expect(visitBtn).toBeVisible();
        const { fontSize, fontWeight } = await visitBtn.evaluate((el) => {
            const cs = getComputedStyle(el);
            return { fontSize: parseFloat(cs.fontSize), fontWeight: parseInt(cs.fontWeight, 10) };
        });
        expect(fontSize).toBeGreaterThanOrEqual(18);
        expect(fontWeight).toBeGreaterThanOrEqual(700);

        await assertCleanConsole(msgs);
    });
});
