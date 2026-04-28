import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole } from './helpers';

// Smoke-tagged so this also runs against dev / prod after deploy. That's the
// whole point: a post-deploy version mismatch (frontend label ≠ /api/version)
// means we shipped half the build, and the user is looking at stale UI.
const BASE = process.env.SMOKE_BASE_URL || 'http://localhost:8099';

test.describe('Version display @smoke', () => {
    test('dashboard shows v<semver> label that matches /api/version', async ({ page, request }) => {
        const msgs = setupConsoleCheck(page);

        // Public route — works pre-login. The version label is mounted in the
        // App shell so it shows on every route, including /login.
        await page.goto(`${BASE}/login`);

        const label = page.locator('[data-testid="version"]');
        await expect(label).toBeVisible({ timeout: 15000 });

        const labelText = (await label.textContent())?.trim() ?? '';
        // Format: v<major>.<minor>.<patch>(-dev.<n>)? — matches the spec in
        // version-on-dashboard.md. Optional short SHA / date suffix allowed
        // for future build-time injection upgrades.
        expect(labelText).toMatch(/^v\d+\.\d+\.\d+(-dev\.\d+)?(\s\([0-9a-f]{7}(,\s\d{4}-\d{2}-\d{2})?\))?$/);

        // Frontend label is build-time-injected from spinbike-ui/Cargo.toml's
        // version field. Backend /api/version reads its own CARGO_PKG_VERSION
        // from the same VERSION-synced source. They must agree on every build
        // — drift means a partial deploy.
        const apiResp = await request.get(`${BASE}/api/version`);
        expect(apiResp.ok()).toBe(true);
        const apiBody = await apiResp.json();
        expect(typeof apiBody.version).toBe('string');
        expect(labelText).toBe(`v${apiBody.version}`);

        assertCleanConsole(msgs);
    });

    test('version label is present on /staff (every-route check) @smoke', async ({ page }) => {
        const msgs = setupConsoleCheck(page);
        // /staff redirects unauthenticated users to /login; the label must be
        // visible at both URLs because it lives in the App shell, not behind
        // the route guard.
        await page.goto(`${BASE}/staff`);
        const label = page.locator('[data-testid="version"]');
        await expect(label).toBeVisible({ timeout: 15000 });
        assertCleanConsole(msgs);
    });
});
