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

        // Ordering check via the API (UI page-1 may not contain all three rows
        // when the shared E2E DB has many no-movement users from prior tests).
        const listResp = await fetch(
            `${BASE_URL}/api/users/by-last-movement?limit=200&offset=0`,
            { headers: { Authorization: `Bearer ${token}` } },
        );
        if (!listResp.ok) throw new Error(`list failed: ${listResp.status}`);
        const listAll = (await listResp.json()) as Array<{ id: number }>;
        const idxA = listAll.findIndex((r) => r.id === a.user_id);
        const idxB = listAll.findIndex((r) => r.id === b.user_id);
        const idxC = listAll.findIndex((r) => r.id === c.user_id);
        expect(idxA).toBeGreaterThanOrEqual(0);
        expect(idxB).toBeGreaterThanOrEqual(0);
        expect(idxC).toBeGreaterThanOrEqual(0);
        expect(idxA).toBeLessThan(idxB);
        expect(idxB).toBeLessThan(idxC);

        // Navigate to Reports → Users tab.
        await page.goto('/reports');
        await page.click('[data-testid="reports-tab-users"]');
        await expect(page.locator('[data-testid="users-by-movement"]')).toBeVisible();

        // B has dated activity (2 days ago) so it sits early in the dated
        // section. UMA-A (no movement) is in the NULL-FIRST chunk; UMA-C may
        // be on a later page. Click "Show more" until B's row appears (max 5
        // pages = 250 rows). The page-1 cap is 50 rows in the component.
        const rowB = page.locator(`[data-testid="user-row"]:has-text("${b.name}")`);
        for (let i = 0; i < 5; i++) {
            if (await rowB.count()) break;
            const showMore = page.locator('[data-testid="users-by-movement-show-more"]');
            if (!(await showMore.count())) break;
            await showMore.click();
            await page.waitForTimeout(200);
        }
        await expect(rowB).toBeVisible();

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

        // After delete, confirm B is gone via API (cheaper than re-paginating
        // the UI). Search the full list (200 rows) which more than covers any
        // accumulated no-movement users from sibling tests.
        const postDeleteResp = await fetch(
            `${BASE_URL}/api/users/by-last-movement?limit=200&offset=0`,
            { headers: { Authorization: `Bearer ${token}` } },
        );
        const postDeleteList = (await postDeleteResp.json()) as Array<{ id: number }>;
        expect(postDeleteList.find((r) => r.id === b.user_id)).toBeUndefined();

        assertCleanConsole(msgs);
    });
});
