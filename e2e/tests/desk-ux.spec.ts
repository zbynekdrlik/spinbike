import { test, expect, Page } from '@playwright/test';
import {
    setupConsoleCheck,
    assertCleanConsole,
    loginViaAPI,
    activateUniqueCard,
} from './helpers';

const BASE_URL = 'http://localhost:8099';

async function sellPassToUser(
    token: string,
    userId: number,
    daysFromToday: number,
): Promise<void> {
    const validUntil = new Date(Date.now() + daysFromToday * 86400e3)
        .toISOString()
        .slice(0, 10);
    const resp = await fetch(`${BASE_URL}/api/payments/sell-pass`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ user_id: userId, price: 35.0, valid_until: validUntil }),
    });
    if (!resp.ok) throw new Error(`sell-pass failed: ${resp.status} ${await resp.text()}`);
}

async function lookupUserId(token: string, cardCode: string): Promise<number> {
    const resp = await fetch(
        `${BASE_URL}/api/users/lookup/${encodeURIComponent(cardCode)}`,
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

async function getSpinningService(token: string): Promise<{ id: number; default_price: number; active: number }> {
    // GET /api/admin/services requires staff role (or admin); both seeded test
    // users have it. No separate /api/services endpoint exists.
    const resp = await fetch(`${BASE_URL}/api/admin/services`, {
        headers: { Authorization: `Bearer ${token}` },
    });
    if (!resp.ok) throw new Error(`GET /api/admin/services failed: ${resp.status} ${await resp.text()}`);
    const all = await resp.json();
    const spinning = all.find((s: { name_en: string }) => s.name_en === 'Spinning');
    if (!spinning) throw new Error('Spinning service not found in /api/admin/services response');
    return spinning as { id: number; default_price: number; active: number };
}

async function setSpinningActive(adminToken: string, svcId: number, active: boolean): Promise<void> {
    // PUT /api/admin/services/{id} accepts partial updates; send only the
    // active flag. The server's UpdateServiceRequest expects active: bool
    // (NOT 0|1) and merges with existing values.
    const put = await fetch(`${BASE_URL}/api/admin/services/${svcId}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
        body: JSON.stringify({ active }),
    });
    if (!put.ok) throw new Error(`PUT /api/admin/services/${svcId} failed: ${put.status} ${await put.text()}`);
}

test.describe('Staff desk UX cluster — issues #29 #30 #31 #32 #34', () => {
    test('card header shows name and barcode on one line', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName, barcode } = await activateUniqueCard(token, 50.0, 'UX');
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

        assertCleanConsole(msgs);
    });

    test('pass banner active is one line', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName, barcode } = await activateUniqueCard(token, 100.0, 'UX');
        const userId = await lookupUserId(token, barcode);
        await sellPassToUser(token, userId, 14);
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        const banner = page.locator('[data-testid="pass-banner-active"]');
        await expect(banner).toBeVisible();
        await expect(banner).toHaveText(/^✓ Monthly pass valid until \d{4}-\d{2}-\d{2} \(\d+ days\)/);

        // Pencil edit button is present and inside the same single-line container.
        const editBtn = banner.locator('[data-testid="pass-date-edit"]');
        await expect(editBtn).toBeVisible();
        // No legacy `.pass-banner-sub` div.
        await expect(banner.locator('.pass-banner-sub')).toHaveCount(0);

        assertCleanConsole(msgs);
    });

    test('pass banner expired is one line', async ({ page, request }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const suffix = Array.from({ length: 8 }, () =>
            String.fromCharCode(97 + Math.floor(Math.random() * 26)),
        ).join('');
        const cardBarcode = `UX-EXP-${suffix}`;

        // Seed an expired-pass card via the test fixture endpoint (bypasses
        // the server-side valid_until > today validation).
        const seedResp = await request.post(`${BASE_URL}/api/test/seed-expired-pass`, {
            data: { barcode: cardBarcode, valid_until: '2020-01-01' },
            headers: { Authorization: `Bearer ${token}` },
        });
        expect(seedResp.ok()).toBeTruthy();

        await page.goto('/staff');
        const searchInput = page.locator('input[type="search"]');
        await searchInput.focus();
        await page.keyboard.type(cardBarcode, { delay: 30 });
        await page.locator('[data-testid="search-result"]').first().click();

        const banner = page.locator('[data-testid="pass-banner-expired"]');
        await expect(banner).toBeVisible();
        // Symmetric guard: expired must also be a single line, no .pass-banner-sub.
        await expect(banner.locator('.pass-banner-sub')).toHaveCount(0);

        assertCleanConsole(msgs);
    });

    test('Cards — Quick Dashboard h1 is gone', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        await page.goto('/staff');
        await expect(page.locator('h1.page-title')).toHaveCount(0);
        const body = (await page.locator('body').textContent()) ?? '';
        expect(body.toLowerCase()).not.toContain('cards — quick dashboard');
        expect(body.toLowerCase()).not.toContain('karty — rychly prehlad');

        assertCleanConsole(msgs);
    });

    test('log-visit class buttons are bigger and bolder', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName, barcode } = await activateUniqueCard(token, 100.0, 'UX');
        const userId = await lookupUserId(token, barcode);
        await sellPassToUser(token, userId, 14);
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

        assertCleanConsole(msgs);
    });

    test('Fitness preselected when staff opens a card (#33)', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0, 'UX');
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        // ActionForm renders inside the action-panel.
        await expect(page.locator('[data-testid="action-form"]')).toBeVisible();

        // The <select> must show "Fitness" as the active option's visible
        // text. Use auto-retrying assertion so the test waits deterministically
        // for the preselect Effect to land rather than racing it on a one-shot
        // textContent() read. "Fitness" is identical in Slovak and English, so
        // no language forcing is needed.
        await expect(
            page.locator('[data-testid="charge-service"] option:checked'),
        ).toHaveText('Fitness');

        assertCleanConsole(msgs);
    });

    test('Empty option is not the active selection when Fitness preselect succeeds (#33)', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0, 'UX');
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        await expect(page.locator('[data-testid="action-form"]')).toBeVisible();

        // The <select> value (which is the option's `value=` attribute, i.e.
        // the service id) must be a non-empty string that parses to a positive
        // integer. The empty <option value=""> placeholder still exists in the
        // DOM as the missing-Fitness fallback, but it must NOT be the active
        // selection in this normal-case test. Use auto-retrying not.toHaveValue
        // so the test waits for the preselect Effect rather than racing it.
        await expect(page.locator('[data-testid="charge-service"]')).not.toHaveValue('');
        const value = await page.locator('[data-testid="charge-service"]').inputValue();
        const parsed = Number.parseInt(value, 10);
        expect(Number.isFinite(parsed)).toBe(true);
        expect(parsed).toBeGreaterThan(0);

        assertCleanConsole(msgs);
    });

    test('Spinning chip charges card in one click (#34)', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const spinning = await getSpinningService(token);
        const { lastName } = await activateUniqueCard(token, 50.0, 'UX');
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        await expect(page.locator('[data-testid="action-form"]')).toBeVisible();

        const chip = page.locator('[data-testid="quick-charge-spinning"]');
        await expect(chip).toBeVisible();
        await expect(chip).toContainText(`Spinning ${spinning.default_price.toFixed(2)} €`);

        // Capture credit BEFORE the click — the credit reading lives in
        // [data-testid="card-credit"] inside the action panel header.
        const creditBefore = parseFloat(
            (await page.locator('[data-testid="card-credit"]').textContent()) ?? '0',
        );

        await chip.click();

        // After charge: txn list populated, empty-state absent, credit decreased.
        // Use auto-retrying assertions throughout — the txn list re-renders
        // asynchronously after set_txn_refresh + API fetch; one-shot reads
        // race the re-render and yield 0 rows transiently.
        await expect(page.locator('[data-testid="transactions-list"]')).toBeVisible();
        await expect(page.locator('[data-testid="transactions-list-empty"]')).toHaveCount(0);
        await expect(page.locator('[data-testid="transaction-row"]')).not.toHaveCount(0);

        await expect
            .poll(async () => parseFloat((await page.locator('[data-testid="card-credit"]').textContent()) ?? '0'))
            .toBeCloseTo(creditBefore - spinning.default_price, 2);

        assertCleanConsole(msgs);
    });

    test('Spinning chip is absent when service is inactive (#34)', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const adminToken = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        const spinning = await getSpinningService(adminToken);

        // Deactivate Spinning. Use try/finally so the service is reactivated
        // even if assertions throw — leaking active=0 would break unrelated
        // tests on shared CI state.
        await setSpinningActive(adminToken, spinning.id, false);
        try {
            const staffToken = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
            const { lastName } = await activateUniqueCard(staffToken, 50.0, 'UX');
            await page.goto('/staff');
            await openCardByLastName(page, lastName);

            await expect(page.locator('[data-testid="action-form"]')).toBeVisible();

            await expect(page.locator('[data-testid="quick-charge-spinning"]')).toHaveCount(0);
            await expect(page.locator('[data-testid="charge-submit"]')).toBeVisible();
        } finally {
            await setSpinningActive(adminToken, spinning.id, true);
        }

        assertCleanConsole(msgs);
    });

    test('Regression fence: txn list still populates after Spinning chip charge (#34)', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { lastName } = await activateUniqueCard(token, 50.0, 'UX');
        await page.goto('/staff');
        await openCardByLastName(page, lastName);

        await expect(page.locator('[data-testid="action-form"]')).toBeVisible();
        await page.locator('[data-testid="quick-charge-spinning"]').click();

        // The exact regression class from PR #35: empty-state must NOT appear,
        // and at least one row must be present. Both via auto-retrying
        // assertions so we don't race the post-charge re-render.
        await expect(page.locator('[data-testid="transactions-list-empty"]')).toHaveCount(0);
        await expect(page.locator('[data-testid="transaction-row"]')).not.toHaveCount(0);

        assertCleanConsole(msgs);
    });
});
