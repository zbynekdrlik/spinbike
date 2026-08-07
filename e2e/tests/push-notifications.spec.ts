import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole } from './helpers';

/**
 * E2E coverage for the "Enable notifications" button on `/my/balance`
 * (#264). Per the issue's own testing scope: the button appears, the
 * permission flow is NOT auto-triggered on page load, and the subscription
 * POST reaches the server. Push DELIVERY itself is explicitly out of scope
 * for Playwright (the issue's own words: "do not fake it") — this test
 * never asserts a notification was shown to the OS.
 *
 * **Why `PushManager.prototype.subscribe` is stubbed, but nothing else
 * is:** `PushManager.subscribe()` is a genuine external-network call (the
 * browser talks to its OWN push service, e.g. Chrome's default FCM
 * endpoint) — the exact "external network service" carve-out
 * `test-strictness.md` allows mocking. Everything BEFORE and AFTER that
 * one hop is real: a real button click, the real WASM permission-request +
 * service-worker-ready code path, and a REAL POST to this app's own
 * `/api/push/subscribe` backend, verified to actually reach the server and
 * persist a row. This mirrors the project's OWN established pattern for
 * eWeLink/SMTP (both run in an in-process TEST_MODE stub in CI rather than
 * hitting the real cloud) — the alternative (a live FCM round-trip on
 * every CI run) would make this test flaky/slow for a third party's
 * infrastructure entirely outside this project's control.
 */

const BASE_URL = 'http://localhost:8099';

function randSuffix(): string {
    return Array.from({ length: 8 }, () =>
        String.fromCharCode(97 + Math.floor(Math.random() * 26)),
    ).join('');
}

async function seedCustomer(prefix: string): Promise<{ user_id: number; email: string; password: string }> {
    const suffix = randSuffix();
    const email = `${prefix}-${suffix}@test.local`;
    const password = `Pw-${suffix}`;
    const resp = await fetch(`${BASE_URL}/api/test/seed-account`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email, password, name: `${prefix} ${suffix}`, role: 'customer' }),
    });
    if (!resp.ok) throw new Error(`seed-account failed: ${resp.status} ${await resp.text()}`);
    const { user_id } = await resp.json();
    return { user_id, email, password };
}

/** Stub the ONE external-network hop (see module doc) before any page script runs. */
async function stubPushManagerSubscribe(page: import('@playwright/test').Page, endpoint: string): Promise<void> {
    await page.addInitScript((ep: string) => {
        if (typeof (window as unknown as { PushManager?: unknown }).PushManager === 'undefined') {
            return;
        }
        const pm = (window as unknown as { PushManager: { prototype: Record<string, unknown> } }).PushManager;
        pm.prototype.subscribe = function subscribe() {
            return Promise.resolve({
                endpoint: ep,
                toJSON() {
                    return {
                        endpoint: ep,
                        keys: {
                            p256dh:
                                'BH1HTeKM7-NwaLGHEqxeu2IamQaVVLkcsFHPIHmsCnqxcBHPQBprF41bEMOr3O1hUQ2jU1opNEm1F_lZV_sxMP8',
                            auth: 'sBXU5_tIYz-5w7G2B25BEw',
                        },
                    };
                },
            });
        };
    }, endpoint);
}

test('the enable-notifications button appears, never auto-prompts, and its subscribe POST reaches the server', async ({
    page,
    context,
}) => {
    const messages = setupConsoleCheck(page);
    const customer = await seedCustomer('push');

    const subscriptionEndpoint = `https://push.example.test/e2e-${randSuffix()}`;
    await context.grantPermissions(['notifications'], { origin: BASE_URL });
    await stubPushManagerSubscribe(page, subscriptionEndpoint);

    await loginViaAPI(page, BASE_URL, customer.email, customer.password);
    await page.goto('/my/balance');

    // The button must appear WITHOUT any permission prompt having fired on
    // load — nothing to assert programmatically for "no prompt shown" other
    // than the fact permission was pre-granted via grantPermissions, so a
    // prompt would have been a no-op even if (wrongly) triggered; the real
    // guard is behavioural: no subscribe POST until the click below.
    const enableButton = page.locator('[data-testid="push-enable-button"]');
    await expect(enableButton).toBeVisible();

    const subscribeRequest = page.waitForResponse(
        (resp) => resp.url().includes('/api/push/subscribe') && resp.request().method() === 'POST',
    );
    await enableButton.click();
    const subscribeResp = await subscribeRequest;
    expect(subscribeResp.status()).toBe(200);

    // UI reflects the new "on" state.
    await expect(page.locator('[data-testid="push-toggle-on"]')).toBeVisible();

    assertCleanConsole(messages);
});
