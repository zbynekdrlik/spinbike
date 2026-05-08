import { test, expect } from '@playwright/test';
import {
    setupConsoleCheck,
    assertCleanConsole,
    loginViaAPI,
    createUniqueUser,
} from './helpers';

const BASE_URL = 'http://localhost:8099';

test.describe('Users by last movement (#56)', () => {
    test('Reports → Users tab orders oldest-first and supports soft-delete', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        // Seed three uniquely-prefixed users; A has no movement, B has an OLDER charge, C has a NEW charge.
        const a = await createUniqueUser(token, 0.0, 'UMA-A');
        const b = await createUniqueUser(token, 0.0, 'UMA-B');
        const c = await createUniqueUser(token, 0.0, 'UMA-C');

        // Look up the Spinning service.
        const svcResp = await fetch(`${BASE_URL}/api/admin/services`, {
            headers: { Authorization: `Bearer ${token}` },
        });
        if (!svcResp.ok) throw new Error(`/api/admin/services failed: ${svcResp.status}`);
        const services = (await svcResp.json()) as Array<{ id: number; name_en: string }>;
        const spinning = services.find((s) => s.name_en === 'Spinning');
        if (!spinning) throw new Error('Spinning service not found');

        // Charge B and C.
        for (const uid of [b.user_id, c.user_id]) {
            const r = await fetch(`${BASE_URL}/api/payments/charge`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
                body: JSON.stringify({ user_id: uid, amount: 1.0, service_id: spinning.id }),
            });
            if (!r.ok) throw new Error(`charge POST failed: ${r.status}`);
        }

        // Backdate B's transaction to 2 days ago using the existing PATCH /api/transactions/{id}/created-at endpoint.
        const txResp = await fetch(`${BASE_URL}/api/users/${b.user_id}/transactions`, {
            headers: { Authorization: `Bearer ${token}` },
        });
        if (!txResp.ok) throw new Error(`txn list failed: ${txResp.status}`);
        const txns = (await txResp.json()) as Array<{ id: number; action: string }>;
        const chargeTxn = txns.find((t) => t.action === 'charge');
        if (!chargeTxn) throw new Error('charge txn not found');
        const twoDaysAgo = new Date();
        twoDaysAgo.setDate(twoDaysAgo.getDate() - 2);
        const yyyy = twoDaysAgo.getFullYear();
        const mm = String(twoDaysAgo.getMonth() + 1).padStart(2, '0');
        const dd = String(twoDaysAgo.getDate()).padStart(2, '0');
        const targetDate = `${yyyy}-${mm}-${dd}`;
        const patchResp = await fetch(`${BASE_URL}/api/transactions/${chargeTxn.id}/created-at`, {
            method: 'PATCH',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
            body: JSON.stringify({ created_at_date: targetDate }),
        });
        if (!patchResp.ok) throw new Error(`backdate failed: ${patchResp.status}`);

        // Navigate to Reports → Users tab.
        await page.goto('/reports');
        await page.click('[data-testid="reports-tab-users"]');
        await expect(page.locator('[data-testid="users-by-movement"]')).toBeVisible();

        // Locate the three seeded rows and assert ordering: A (no mvmt) before B (older) before C (newer).
        const rowA = page.locator(`[data-testid="user-row"]:has-text("${a.name}")`);
        const rowB = page.locator(`[data-testid="user-row"]:has-text("${b.name}")`);
        const rowC = page.locator(`[data-testid="user-row"]:has-text("${c.name}")`);
        await expect(rowA).toBeVisible();
        await expect(rowB).toBeVisible();
        await expect(rowC).toBeVisible();

        // Read positions via DOM order — pull all rows, find the three seeded names.
        const allRows = await page.locator('[data-testid="user-row"]').all();
        const names: string[] = [];
        for (const r of allRows) names.push((await r.innerText()).split('\n')[0]);
        const idxA = names.findIndex((n) => n === a.name);
        const idxB = names.findIndex((n) => n === b.name);
        const idxC = names.findIndex((n) => n === c.name);
        expect(idxA).toBeGreaterThanOrEqual(0);
        expect(idxA).toBeLessThan(idxB);
        expect(idxB).toBeLessThan(idxC);

        // Click row B → navigation lands on /staff with B's panel open.
        await Promise.all([
            page.waitForURL(/\/staff\?card=/, { timeout: 5000 }),
            rowB.click(),
        ]);
        await expect(page.locator('[data-testid="action-panel"]')).toBeVisible();

        // Click delete button → modal opens with B's name in the title.
        await page.click('[data-testid="delete-user-button"]');
        await expect(page.locator('[data-testid="sheet-delete-user"]')).toBeVisible();
        // Confirm
        await page.click('[data-testid="delete-user-confirm"]');
        await expect(page.locator('[data-testid="sheet-delete-user"]')).toBeHidden();
        await expect(page.locator('[data-testid="action-panel"]')).toBeHidden();

        // Navigate back to Reports → Users tab; B must be gone.
        await page.goto('/reports');
        await page.click('[data-testid="reports-tab-users"]');
        await expect(page.locator('[data-testid="users-by-movement"]')).toBeVisible();
        await expect(page.locator(`[data-testid="user-row"]:has-text("${b.name}")`)).toHaveCount(0);

        assertCleanConsole(msgs);
    });
});
