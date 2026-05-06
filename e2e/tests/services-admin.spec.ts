import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Admin services — dual-language CRUD', () => {
    test('creates a generic service with Slovak + English names', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/admin');
        await page.waitForSelector('h1.page-title');
        await page.locator('[data-testid="admin-tab-services"]').click();
        await page.waitForFunction(() => !document.querySelector('.spinner'), { timeout: 10000 });

        const suffix = `${Date.now()}`;
        const skName = `TestSk${suffix}`;
        const enName = `TestEn${suffix}`;

        await page.locator('[data-testid="service-name-sk-input"]').fill(skName);
        await page.locator('[data-testid="service-name-en-input"]').fill(enName);
        await page.locator('[data-testid="service-price-input"]').fill('1.50');
        await page.locator('[data-testid="service-create-btn"]').click();

        // The new row appears in the list. Both names are columns; the kind
        // column shows "generic" (or its localized label).
        await expect(page.locator(`text=${skName}`)).toBeVisible();
        await expect(page.locator(`text=${enName}`)).toBeVisible();

        assertCleanConsole(msgs);
    });

    test('the Monthly pass option in the kind selector is disabled when one exists', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        await page.goto('/admin');
        await page.waitForSelector('h1.page-title');
        await page.locator('[data-testid="admin-tab-services"]').click();
        await page.waitForFunction(() => !document.querySelector('.spinner'), { timeout: 10000 });

        // V8 seeds Monthly pass — selector should disable that option.
        await page.locator('[data-testid="service-kind-select"]').waitFor();
        const passOption = page.locator(
            '[data-testid="service-kind-select"] option[value="monthly_pass"]'
        );
        await expect(passOption).toBeDisabled();

        assertCleanConsole(msgs);
    });

    test('GET /api/admin/services returns rows with kind, name_sk, name_en', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        const resp = await fetch(`${BASE_URL}/api/admin/services`, {
            headers: { Authorization: `Bearer ${token}` },
        });
        expect(resp.status).toBe(200);
        const rows = await resp.json();

        // V8 seeds at least: Spinning, Fitness, Mesačná permanentka (monthly_pass),
        // Občerstvenie, Doplnky výživy, Aktivácia karty.
        const byKind = rows.filter((r: { kind: string }) => r.kind === 'monthly_pass');
        expect(byKind.length).toBe(1);
        expect(byKind[0].name_sk).toBe('Mesačná permanentka');
        expect(byKind[0].name_en).toBe('Monthly pass');

        const skNames = rows.map((r: { name_sk: string }) => r.name_sk);
        expect(skNames).toContain('Občerstvenie');
        expect(skNames).toContain('Doplnky výživy');
        expect(skNames).toContain('Aktivácia karty');

        // Also drive the page so the console-check has DOM context.
        await page.goto('/admin?tab=services');
        assertCleanConsole(msgs);
    });
});
