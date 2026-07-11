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

    test('the single_entry service (door self-entry, formerly Fitness) shows a real label, not ??? (#186)', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');
        // loginViaAPI defaults localStorage lang to 'en'; force Slovak so we
        // assert the actual Slovak label the owner sees (#186's real fix).
        await page.addInitScript(() => {
            try { localStorage.setItem('spinbike_lang', 'sk'); } catch { /* storage not ready */ }
        });
        await page.goto('/admin');
        await page.waitForSelector('h1.page-title');
        await page.locator('[data-testid="admin-tab-services"]').click();
        await page.waitForFunction(() => !document.querySelector('.spinner'), { timeout: 10000 });

        // Migration V16 re-tags the seeded "Fitness" row to kind='single_entry'
        // for the door self-entry feature. Its kind badge used to render "???"
        // because i18n.rs had no service_kind_single_entry key.
        const row = page.locator('tr', { hasText: 'Fitness' });
        await expect(row).toBeVisible();
        await expect(row.locator('.badge--single_entry')).toBeVisible();
        await expect(row.locator('.badge--single_entry')).toHaveText('Jednorazovy vstup');
        await expect(row.locator('.badge--single_entry')).not.toHaveText('???');

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
