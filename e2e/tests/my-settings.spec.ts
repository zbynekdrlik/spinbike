import { test, expect } from '@playwright/test';
import {
    loginViaAPI,
    setupConsoleCheck,
    assertCleanConsole,
    seedCustomerAccount,
    pushFakeEndpoint,
    stubPushGrantedAndSubscribable,
    stubPushDenied,
} from './helpers';

/**
 * #316: the customer-facing settings screen at `/my/settings` — introduced
 * so the push-notification toggle has somewhere to live once it's hidden
 * from `/my/balance` for the (already-on) `On`/`Busy` states. See
 * `push-notifications.spec.ts`'s module doc for the full push-toggle
 * subscribe/unsubscribe coverage — this file covers only the NEW surface
 * split: is `/my/settings` reachable, does it show the full toggle, and
 * does `/my/balance` correctly hide the row once notifications are on.
 */

const BASE_URL = 'http://localhost:8099';

test('a customer reaches /my/settings from the navbar and it shows the push toggle', async ({ page }) => {
    const messages = setupConsoleCheck(page);
    const customer = await seedCustomerAccount(BASE_URL, 'settings-nav');

    // Blocked state (denied permission) — deterministic, renders on BOTH
    // surfaces, and needs no subscribe-flow stubbing at all.
    await stubPushDenied(page);
    await loginViaAPI(page, BASE_URL, customer.email, customer.password);

    await page.goto('/my/balance');
    // #319: the Settings link moved behind the customer burger menu.
    await page.locator('[data-testid="navbar-burger"]').click();
    const settingsLink = page.locator('[data-testid="menu-settings"]');
    await expect(settingsLink).toBeVisible();
    await settingsLink.click();
    await page.waitForURL('**/my/settings');

    await expect(page.locator('[data-testid="push-toggle-switch"]')).toBeVisible();
    await expect(page.locator('[data-testid="push-toggle-blocked"]')).toBeVisible();

    assertCleanConsole(messages);
});

test('/my/balance does not show the push toggle once notifications are already on', async ({ page, context }) => {
    const messages = setupConsoleCheck(page);
    const customer = await seedCustomerAccount(BASE_URL, 'settings-hide');
    const endpoint = pushFakeEndpoint();

    await context.grantPermissions(['notifications'], { origin: BASE_URL });
    await stubPushGrantedAndSubscribable(page, endpoint);
    await loginViaAPI(page, BASE_URL, customer.email, customer.password);

    // Auto-subscribes on mount (permission already granted, no server
    // subscription yet) — wait for the real subscribe POST so the state is
    // genuinely "On" before asserting the row is gone.
    const subscribeRequest = page.waitForResponse(
        (resp) => resp.url().includes('/api/push/subscribe') && resp.request().method() === 'POST',
    );
    await page.goto('/my/balance');
    await subscribeRequest;

    // Give the reactive re-render a moment to settle, then assert the row
    // is genuinely absent (not just not-yet-rendered).
    await expect(page.locator('[data-testid="push-toggle-on"]')).toHaveCount(0);
    await expect(page.locator('[data-testid="push-toggle-switch"]')).toHaveCount(0);

    // The full toggle is still reachable and shows "On" on /my/settings —
    // proves the state genuinely settled to On, not e.g. stuck Loading.
    await page.goto('/my/settings');
    await expect(page.locator('[data-testid="push-toggle-on"]')).toBeVisible();
    await expect(page.locator('[data-testid="push-toggle-switch"]')).toHaveAttribute('aria-checked', 'true');

    assertCleanConsole(messages);
});
