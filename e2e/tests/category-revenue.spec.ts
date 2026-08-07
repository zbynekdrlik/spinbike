import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole, bratislavaToday, uniqueLetterSuffix } from './helpers';

const BASE_URL = 'http://localhost:8099';

interface CategoryRevenueRow {
    service_id: number;
    name_sk: string;
    name_en: string;
    total_eur: number;
}

interface ServiceLookup {
    supplements: number;
    fitness: number;
}

async function fetchServiceIds(token: string): Promise<ServiceLookup> {
    const resp = await fetch(`${BASE_URL}/api/admin/services`, {
        headers: { Authorization: `Bearer ${token}` },
    });
    if (!resp.ok) throw new Error(`/api/admin/services failed: ${resp.status}`);
    const services: { id: number; name_en: string }[] = await resp.json();
    const find = (n: string) => {
        const s = services.find((x) => x.name_en === n);
        if (!s) throw new Error(`service "${n}" not in /api/admin/services`);
        return s.id;
    };
    return { supplements: find('Supplements'), fitness: find('Fitness') };
}

async function createUser(token: string, suffix: string, credit: number): Promise<number> {
    const resp = await fetch(`${BASE_URL}/api/users`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({
            name: `CR Reports${suffix}`,
            initial_credit: credit,
            card_code: `CR-${suffix}`,
        }),
    });
    if (!resp.ok) throw new Error(`createUser failed: ${resp.status} ${await resp.text()}`);
    const user = await resp.json();
    return user.id;
}

async function postCharge(token: string, userId: number, serviceId: number, amount: number) {
    const resp = await fetch(`${BASE_URL}/api/payments/charge`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ user_id: userId, amount, service_id: serviceId }),
    });
    if (!resp.ok) throw new Error(`charge failed: ${resp.status} ${await resp.text()}`);
}

function findRow(rows: CategoryRevenueRow[], serviceId: number): CategoryRevenueRow {
    const row = rows.find((r) => r.service_id === serviceId);
    if (!row) throw new Error(`service ${serviceId} missing from category_revenue`);
    return row;
}

// #255 — money stats gained a full revenue-per-category breakdown (Doplnky
// vyzivy/Supplements is one row among all active services, not its own KPI
// tile). Robust against the shared E2E DB the same way reports-attendance.spec.ts
// is: assert the DELTA our own seeded charges produce, not an absolute total.
test.describe('Reports — category revenue breakdown (#255)', () => {
    test('day report category_revenue sums per-service charges; UI section renders rows + total', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        const services = await fetchServiceIds(token);
        // Letters-only suffix (#39 collision class — see helpers.ts) so this
        // card_code can never substring-collide with another spec's short
        // digit search in the shared, single-server E2E DB. This exact
        // suffix (Date.now()-based) is the confirmed root cause of #290's
        // dashboard.spec.ts flake (CI run 31178634087, "CR Reports...").
        const suffix = uniqueLetterSuffix();
        const userId = await createUser(token, suffix, 50.0);

        const today = bratislavaToday();
        const beforeResp = await fetch(`${BASE_URL}/api/reports/day?date=${today}`, {
            headers: { Authorization: `Bearer ${token}` },
        });
        expect(beforeResp.ok).toBe(true);
        const beforeJson = await beforeResp.json();
        const beforeSupplements = findRow(beforeJson.category_revenue, services.supplements).total_eur;
        const beforeFitness = findRow(beforeJson.category_revenue, services.fitness).total_eur;

        // Charges on two different services — proves the breakdown is
        // per-category, not a single lumped total.
        await postCharge(token, userId, services.supplements, 3.5);
        await postCharge(token, userId, services.fitness, 5.0);

        const afterResp = await fetch(`${BASE_URL}/api/reports/day?date=${today}`, {
            headers: { Authorization: `Bearer ${token}` },
        });
        expect(afterResp.ok).toBe(true);
        const afterJson = await afterResp.json();
        const rows: CategoryRevenueRow[] = afterJson.category_revenue;
        const afterSupplements = findRow(rows, services.supplements).total_eur;
        const afterFitness = findRow(rows, services.fitness).total_eur;

        expect(afterSupplements - beforeSupplements).toBeCloseTo(3.5, 2);
        expect(afterFitness - beforeFitness).toBeCloseTo(5.0, 2);

        // Every active service appears (LEFT JOIN — including ones with zero
        // sales today), sorted by total_eur DESC.
        expect(rows.length).toBeGreaterThanOrEqual(2);
        for (let i = 1; i < rows.length; i++) {
            expect(rows[i - 1].total_eur).toBeGreaterThanOrEqual(rows[i].total_eur);
        }

        // Drive the UI: the /reports page defaults to "today" (Day mode),
        // matching the API probe above.
        await page.goto('/reports');
        await expect(page.locator('[data-testid="category-revenue"]')).toBeVisible();
        const categoryRows = page.locator('[data-testid="category-revenue-row"]');
        await expect(categoryRows).toHaveCount(rows.length);
        await expect(page.locator('[data-testid="category-revenue"]')).toContainText('€');
        await expect(page.locator('[data-testid="category-revenue-total"]')).toBeVisible();
        await expect(page.locator('[data-testid="category-revenue-total"]')).toContainText('€');

        assertCleanConsole(consoleMessages);
    });
});
