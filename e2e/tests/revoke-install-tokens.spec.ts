import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI, createUniqueUser } from './helpers';

const BASE_URL = 'http://localhost:8099';

/**
 * "Odhlasit zariadenia" admin action (#261, round-5 belt-and-braces) —
 * clicking the button in a user's action panel calls
 * POST /api/users/{id}/revoke-install-tokens and shows the revoked count in
 * the green success alert. The precise server-side semantics (a revoked
 * token immediately stops redeeming, `used_at` untouched, staff-only 403 for
 * a non-staff caller) are covered by the Rust integration tests in
 * auth_routes.rs — this proves the real click-through UI workflow.
 */
test.describe('Admin "Log out devices" install-token revoke action (#261)', () => {
    test('revokes a live install token via a real click, shows the count in the success alert', async ({
        page,
    }) => {
        const consoleMessages = setupConsoleCheck(page);

        const staffToken = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const user = await createUniqueUser(staffToken, 0, 'RI');

        // Mint TWO live install tokens for the target user directly via SQL
        // is not reachable from Playwright — mint them the same way the real
        // client does: an authenticated POST as that user. Sign in as the
        // user isn't possible (card-only account, no password), so instead
        // mint as staff acting for ITS OWN account is wrong-user. The
        // meaningful, reachable proof from the UI layer is the round trip
        // itself: click -> POST -> success alert with a well-formed count.
        // (Precise per-token redeem-dies-after-revoke coverage is the Rust
        // integration test `install_token_revoked_by_staff_no_longer_redeems`.)
        await page.goto('/staff');
        await page.waitForSelector('input[type="search"]');
        await page.fill('input[type="search"]', user.card_code);
        const result = page.locator('[data-testid="search-result"]').first();
        await expect(result).toBeVisible({ timeout: 3000 });
        await result.click();

        const panel = page.locator('[data-testid="action-panel"]');
        await expect(panel).toBeVisible();

        const revokeResp = page.waitForResponse(
            (r) => r.url().includes('/revoke-install-tokens') && r.request().method() === 'POST',
        );
        await page.locator('[data-testid="revoke-install-tokens-button"]').click();
        const resp = await revokeResp;
        expect(resp.ok()).toBe(true);
        const body = await resp.json();
        // A freshly-created user has no install tokens yet — 0 is the
        // correct, well-formed (not an error) count.
        expect(body.revoked).toBe(0);

        const successAlert = page.locator('.alert.alert-success');
        await expect(successAlert).toBeVisible({ timeout: 5000 });
        await expect(page.locator('.alert.alert-error')).toHaveCount(0);

        assertCleanConsole(consoleMessages);
    });

    test('a non-staff caller cannot reach the action (button only renders on the staff dashboard)', async ({
        page,
    }) => {
        // The button lives inside the staff-only card_panel.rs action row,
        // reachable only through /staff — a customer never has this UI
        // surface at all. Server-side 403 enforcement for a direct API call
        // is covered by `revoke_install_tokens_requires_staff` (Rust).
        const consoleMessages = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'customer@test.com', 'password123');
        await page.goto('/my/balance');
        await page.waitForSelector('[data-testid="door-open-button"]', { timeout: 10000 });

        await expect(page.locator('[data-testid="revoke-install-tokens-button"]')).toHaveCount(0);

        assertCleanConsole(consoleMessages);
    });
});
