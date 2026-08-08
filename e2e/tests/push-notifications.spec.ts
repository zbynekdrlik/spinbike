import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole, uniqueLetterSuffix } from './helpers';

/**
 * E2E coverage for the notification settings row on `/my/balance` (#264,
 * redesigned #303 — a real `role="switch"` toggle replacing the old
 * permanent CTA button, working BOTH ways, plus a silent auto-subscribe and
 * a one-time proactive permission prompt per the owner's #303 follow-up
 * decision). Push DELIVERY itself is explicitly out of scope for
 * Playwright (do not fake it) — these tests never assert a notification
 * was shown to the OS; they exercise the real subscribe/unsubscribe wiring
 * only.
 *
 * **Stubbed via `page.addInitScript`, working around limitations that have
 * nothing to do with this app's correctness:**
 *
 * 1. `PushManager.prototype.subscribe` / `.getSubscription` — genuine
 *    external-network / browser-internal calls (the browser talks to its
 *    OWN push service, or reads its own subscription store) — the exact
 *    "external network service" carve-out `test-strictness.md` allows
 *    mocking. Mirrors the project's own established eWeLink/SMTP TEST_MODE
 *    pattern.
 * 2. `Notification.permission` — a CONFIRMED Playwright/Chromium limitation
 *    (microsoft/playwright#23954, puppeteer/puppeteer#3279):
 *    `context.grantPermissions(['notifications'])` updates
 *    `Notification.requestPermission()`'s resolved value but Blink tracks
 *    the static `Notification.permission` GETTER independently, and it
 *    stays `"denied"` on Playwright's bundled headless Chromium regardless.
 *    Overriding the getter via `Object.defineProperty` (a plain
 *    `static readonly` WebIDL attribute, not `[LegacyUnforgeable]`) is the
 *    standard, well-documented workaround. Tests that need the permission
 *    to start UNDECIDED (`"default"`) deliberately do NOT call
 *    `context.grantPermissions` at all — calling it is exactly what
 *    triggers the getter-stuck-at-`"denied"` bug, so avoiding the call
 *    keeps the real, unstubbed getter at its genuine fresh-context
 *    `"default"` value.
 * 3. `Notification.requestPermission` — stubbed to resolve `"granted"`
 *    immediately ONLY in the one test that exercises the prompt's own
 *    click-to-request flow (real headless Chromium has no UI to answer a
 *    native permission dialog, so an unstubbed call would hang or
 *    auto-deny).
 *
 * Everything else is real: real switch clicks, the real WASM
 * permission-check + service-worker-ready code paths, and REAL POSTs to
 * this app's own `/api/push/subscribe` / `/api/push/unsubscribe` backends,
 * verified to actually reach the server and persist/remove a row.
 */

const BASE_URL = 'http://localhost:8099';

const FAKE_KEYS = {
    p256dh:
        'BH1HTeKM7-NwaLGHEqxeu2IamQaVVLkcsFHPIHmsCnqxcBHPQBprF41bEMOr3O1hUQ2jU1opNEm1F_lZV_sxMP8',
    auth: 'sBXU5_tIYz-5w7G2B25BEw',
};

async function seedCustomer(prefix: string): Promise<{ user_id: number; email: string; password: string }> {
    const suffix = uniqueLetterSuffix();
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

/** Must be on the server's push-endpoint allowlist (routes/push.rs's
 * ALLOWED_PUSH_HOSTS, #264 SSRF-hardening review finding). */
function fakeEndpoint(): string {
    return `https://fcm.googleapis.com/fcm/send/e2e-${uniqueLetterSuffix()}`;
}

/**
 * Permission already GRANTED (stub point 2) + `PushManager.prototype.subscribe`
 * stubbed (stub point 1) — the "auto-subscribe / manual re-enable" environment.
 */
async function stubGrantedAndSubscribable(page: import('@playwright/test').Page, endpoint: string): Promise<void> {
    await page.addInitScript(({ ep, keys }: { ep: string; keys: typeof FAKE_KEYS }) => {
        if (typeof (window as unknown as { Notification?: unknown }).Notification !== 'undefined') {
            Object.defineProperty(Notification, 'permission', {
                configurable: true,
                get: () => 'granted',
            });
        }
        if (typeof (window as unknown as { PushManager?: unknown }).PushManager === 'undefined') {
            return;
        }
        const pm = (window as unknown as { PushManager: { prototype: Record<string, unknown> } }).PushManager;
        pm.prototype.subscribe = function subscribe() {
            return Promise.resolve({
                endpoint: ep,
                toJSON() {
                    return { endpoint: ep, keys };
                },
            });
        };
    }, { ep: endpoint, keys: FAKE_KEYS });
}

/**
 * Permission already GRANTED + `PushManager.prototype.getSubscription`
 * stubbed to return an existing subscription matching `endpoint`, whose
 * `.unsubscribe()` resolves `true` — the "turn off an existing subscription"
 * environment.
 */
async function stubGrantedWithExistingSubscription(page: import('@playwright/test').Page, endpoint: string): Promise<void> {
    await page.addInitScript(({ ep, keys }: { ep: string; keys: typeof FAKE_KEYS }) => {
        if (typeof (window as unknown as { Notification?: unknown }).Notification !== 'undefined') {
            Object.defineProperty(Notification, 'permission', {
                configurable: true,
                get: () => 'granted',
            });
        }
        if (typeof (window as unknown as { PushManager?: unknown }).PushManager === 'undefined') {
            return;
        }
        const pm = (window as unknown as { PushManager: { prototype: Record<string, unknown> } }).PushManager;
        pm.prototype.getSubscription = function getSubscription() {
            return Promise.resolve({
                endpoint: ep,
                toJSON() {
                    return { endpoint: ep, keys };
                },
                unsubscribe() {
                    return Promise.resolve(true);
                },
            });
        };
    }, { ep: endpoint, keys: FAKE_KEYS });
}

/** Permission genuinely `"denied"` (no `context.grantPermissions` call needed). */
async function stubDenied(page: import('@playwright/test').Page): Promise<void> {
    await page.addInitScript(() => {
        if (typeof (window as unknown as { Notification?: unknown }).Notification !== 'undefined') {
            Object.defineProperty(Notification, 'permission', {
                configurable: true,
                get: () => 'denied',
            });
        }
    });
}

/**
 * Permission explicitly forced to `"default"` (undecided) via the SAME
 * `Object.defineProperty` override the other helpers use — headless
 * Chromium's genuine unprompted `Notification.permission` state turned out
 * NOT to be `"default"` in CI (confirmed live: both tests that relied on
 * the real, unstubbed getter failed because the prompt never showed), so
 * this is forced explicitly rather than assumed. No `context.grantPermissions`
 * call (that triggers the getter-stuck-at-`"denied"` Chromium bug, see
 * module doc point 2) — the override below is independent of that bug
 * since it replaces the getter outright rather than relying on the
 * browser's own permission engine.
 */
async function stubUndecided(page: import('@playwright/test').Page): Promise<void> {
    await page.addInitScript(() => {
        if (typeof (window as unknown as { Notification?: unknown }).Notification !== 'undefined') {
            Object.defineProperty(Notification, 'permission', {
                configurable: true,
                get: () => 'default',
            });
        }
    });
}

/**
 * Undecided permission (see `stubUndecided`) plus `Notification.requestPermission`
 * stubbed to resolve `"granted"` immediately (bypasses the native dialog
 * headless Chromium can't answer) and `PushManager.prototype.subscribe`
 * stubbed like the other helpers.
 */
async function stubDefaultWithRequestGranted(page: import('@playwright/test').Page, endpoint: string): Promise<void> {
    await stubUndecided(page);
    await page.addInitScript(({ ep, keys }: { ep: string; keys: typeof FAKE_KEYS }) => {
        if (typeof (window as unknown as { Notification?: unknown }).Notification !== 'undefined') {
            Notification.requestPermission = () => Promise.resolve('granted');
        }
        if (typeof (window as unknown as { PushManager?: unknown }).PushManager === 'undefined') {
            return;
        }
        const pm = (window as unknown as { PushManager: { prototype: Record<string, unknown> } }).PushManager;
        pm.prototype.subscribe = function subscribe() {
            return Promise.resolve({
                endpoint: ep,
                toJSON() {
                    return { endpoint: ep, keys };
                },
            });
        };
    }, { ep: endpoint, keys: FAKE_KEYS });
}

test('permission already granted + no server subscription auto-subscribes on load, with no click', async ({
    page,
    context,
}) => {
    const messages = setupConsoleCheck(page);
    const customer = await seedCustomer('push-auto');
    const endpoint = fakeEndpoint();

    await context.grantPermissions(['notifications'], { origin: BASE_URL });
    await stubGrantedAndSubscribable(page, endpoint);

    await loginViaAPI(page, BASE_URL, customer.email, customer.password);

    const subscribeRequest = page.waitForResponse(
        (resp) => resp.url().includes('/api/push/subscribe') && resp.request().method() === 'POST',
    );
    await page.goto('/my/balance');
    const subscribeResp = await subscribeRequest;
    expect(subscribeResp.status()).toBe(200);

    await expect(page.locator('[data-testid="push-toggle-on"]')).toBeVisible();
    await expect(page.locator('[data-testid="push-toggle-switch"]')).toHaveAttribute('aria-checked', 'true');
    // No proactive prompt when permission is already decided.
    await expect(page.locator('[data-testid="push-prompt"]')).toHaveCount(0);

    assertCleanConsole(messages);
});

test('the switch turns notifications off, unsubscribes, and stays off across a reload', async ({ page, context }) => {
    const messages = setupConsoleCheck(page);
    const customer = await seedCustomer('push-off');
    const endpoint = fakeEndpoint();

    const token = await loginViaAPI(page, BASE_URL, customer.email, customer.password);

    // Pre-seed a server-side subscription (as if the customer had already
    // subscribed in an earlier session) via the real endpoint.
    const seedResp = await fetch(`${BASE_URL}/api/push/subscribe`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ endpoint, keys: FAKE_KEYS }),
    });
    if (!seedResp.ok) throw new Error(`pre-seed subscribe failed: ${seedResp.status} ${await seedResp.text()}`);

    await context.grantPermissions(['notifications'], { origin: BASE_URL });
    await stubGrantedWithExistingSubscription(page, endpoint);

    await page.goto('/my/balance');

    const toggleSwitch = page.locator('[data-testid="push-toggle-switch"]');
    await expect(page.locator('[data-testid="push-toggle-on"]')).toBeVisible();
    await expect(toggleSwitch).toHaveAttribute('aria-checked', 'true');

    const unsubscribeRequest = page.waitForResponse(
        (resp) => resp.url().includes('/api/push/unsubscribe') && resp.request().method() === 'POST',
    );
    await toggleSwitch.click();
    const unsubscribeResp = await unsubscribeRequest;
    expect(unsubscribeResp.status()).toBe(200);

    await expect(page.locator('[data-testid="push-toggle-off"]')).toBeVisible();
    await expect(toggleSwitch).toHaveAttribute('aria-checked', 'false');

    // Permission is still "granted" in the browser after unsubscribing (the
    // switch never touches browser permission, only the subscription) — the
    // auto-subscribe rule must NOT silently re-enable on reload.
    await page.reload();
    await expect(page.locator('[data-testid="push-toggle-off"]')).toBeVisible();
    await expect(page.locator('[data-testid="push-toggle-switch"]')).toHaveAttribute('aria-checked', 'false');

    assertCleanConsole(messages);
});

test('a denied permission shows the blocked state: switch off, disabled, with guidance', async ({ page }) => {
    const messages = setupConsoleCheck(page);
    const customer = await seedCustomer('push-blk');

    await stubDenied(page);
    await loginViaAPI(page, BASE_URL, customer.email, customer.password);
    await page.goto('/my/balance');

    await expect(page.locator('[data-testid="push-toggle-blocked"]')).toBeVisible();
    const toggleSwitch = page.locator('[data-testid="push-toggle-switch"]');
    await expect(toggleSwitch).toHaveAttribute('aria-checked', 'false');
    await expect(toggleSwitch).toBeDisabled();

    assertCleanConsole(messages);
});

test('an undecided permission shows the one-time prompt; dismissing it hides it permanently', async ({ page }) => {
    const messages = setupConsoleCheck(page);
    const customer = await seedCustomer('push-dfl');

    // Force "default" explicitly (see stubUndecided doc) — no
    // context.grantPermissions call, and no subscribe stub, since no
    // subscribe call should ever fire in this test.
    await stubUndecided(page);
    await loginViaAPI(page, BASE_URL, customer.email, customer.password);
    await page.goto('/my/balance');

    await expect(page.locator('[data-testid="push-prompt"]')).toBeVisible();
    await expect(page.locator('[data-testid="push-toggle-off"]')).toBeVisible();
    await expect(page.locator('[data-testid="push-toggle-switch"]')).toHaveAttribute('aria-checked', 'false');

    await page.locator('[data-testid="push-prompt-dismiss"]').click();
    await expect(page.locator('[data-testid="push-prompt"]')).toHaveCount(0);

    await page.reload();
    await expect(page.locator('[data-testid="push-toggle-off"]')).toBeVisible();
    await expect(page.locator('[data-testid="push-prompt"]')).toHaveCount(0);

    assertCleanConsole(messages);
});

test('#305: another device already subscribed does not fool THIS device — it still subscribes for itself', async ({
    page,
    context,
}) => {
    const messages = setupConsoleCheck(page);
    const customer = await seedCustomer('push-multidev');
    const deviceAEndpoint = fakeEndpoint(); // simulates the customer's desktop, already subscribed server-side
    const deviceBEndpoint = fakeEndpoint(); // THIS browser's own, never-yet-subscribed endpoint

    const token = await loginViaAPI(page, BASE_URL, customer.email, customer.password);

    // Pre-seed a server-side subscription for a DIFFERENT endpoint — as if
    // this customer had already subscribed on another device (desktop).
    // GET /api/push/config's `subscribed` flag is a per-ACCOUNT aggregate,
    // so it now reports `true` for the account even though THIS browser
    // (device B) never subscribed.
    const seedResp = await fetch(`${BASE_URL}/api/push/subscribe`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ endpoint: deviceAEndpoint, keys: FAKE_KEYS }),
    });
    if (!seedResp.ok) throw new Error(`pre-seed subscribe failed: ${seedResp.status} ${await seedResp.text()}`);

    // Permission granted + subscribe() stubbed to this device's OWN
    // endpoint — getSubscription() is deliberately left UNSTUBBED (a fresh
    // browser context genuinely has no local subscription yet), so the
    // mount effect's own local-truth check sees the real "nothing here".
    await context.grantPermissions(['notifications'], { origin: BASE_URL });
    await stubGrantedAndSubscribable(page, deviceBEndpoint);

    // If the bug were present, the account-wide `subscribed: true` from
    // device A's row would make the mount effect show "On" for device B
    // with NO subscribe attempt ever made — this wait would time out.
    const subscribeRequest = page.waitForResponse(
        (resp) => resp.url().includes('/api/push/subscribe') && resp.request().method() === 'POST',
    );
    await page.goto('/my/balance');
    const subscribeResp = await subscribeRequest;
    expect(subscribeResp.status()).toBe(200);
    expect(subscribeResp.request().postDataJSON().endpoint).toBe(deviceBEndpoint);

    await expect(page.locator('[data-testid="push-toggle-on"]')).toBeVisible();
    await expect(page.locator('[data-testid="push-toggle-switch"]')).toHaveAttribute('aria-checked', 'true');

    assertCleanConsole(messages);
});

test('clicking "enable" in the one-time prompt requests permission from the click and subscribes', async ({ page }) => {
    const messages = setupConsoleCheck(page);
    const customer = await seedCustomer('push-ask');
    const endpoint = fakeEndpoint();

    await stubDefaultWithRequestGranted(page, endpoint);
    await loginViaAPI(page, BASE_URL, customer.email, customer.password);
    await page.goto('/my/balance');

    await expect(page.locator('[data-testid="push-prompt"]')).toBeVisible();

    const subscribeRequest = page.waitForResponse(
        (resp) => resp.url().includes('/api/push/subscribe') && resp.request().method() === 'POST',
    );
    await page.locator('[data-testid="push-prompt-enable"]').click();
    const subscribeResp = await subscribeRequest;
    expect(subscribeResp.status()).toBe(200);

    await expect(page.locator('[data-testid="push-toggle-on"]')).toBeVisible();
    await expect(page.locator('[data-testid="push-toggle-switch"]')).toHaveAttribute('aria-checked', 'true');
    await expect(page.locator('[data-testid="push-prompt"]')).toHaveCount(0);

    assertCleanConsole(messages);
});
