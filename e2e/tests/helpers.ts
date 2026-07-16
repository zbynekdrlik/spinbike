import { Page, expect } from '@playwright/test';

/**
 * Set up console error/warning collection on a page.
 * Returns an array that accumulates messages during the test.
 *
 * Captures both console.error / console.warning AND uncaught page errors
 * (which is how wasm-bindgen `throw` events surface — they are NOT
 * console events, so listening only to 'console' silently misses every
 * Leptos panic. See #89.)
 */
export function setupConsoleCheck(page: Page): string[] {
    const messages: string[] = [];

    const isFiltered = (text: string): boolean =>
        text.includes('SharedArrayBuffer') ||
        // Trunk bootstrap calls wasm-bindgen init with the legacy
        // positional arg form; wasm-bindgen 0.2.x emits a deprecation
        // warning until Trunk migrates to the single-object form.
        // Not our code's bug; filter until upstream upgrade.
        text.includes('using deprecated parameters for the initialization function') ||
        text.includes('using deprecated parameters for `initSync()`') ||
        // Playwright sometimes navigates away while a previous WASM bundle
        // is still streaming. The browser cancels the in-flight fetch and
        // raises this error. Not a real bug — it is a test-runner artefact.
        text.includes('WebAssembly compilation aborted: Network error') ||
        // The negative-balance list logs every api::get failure (#64) so
        // 3am debugging has signal. Filter ONLY the two specific test-
        // runner artefacts — a generic prefix match would mask real API
        // regressions (e.g. malformed JSON from a deploy) in CI.
        text.includes('negative-balance fetch failed: TypeError: Failed to fetch') ||
        text.includes('negative-balance fetch failed: Missing authorization header') ||
        /the server responded with a status of 4\d\d/.test(text);

    page.on('console', (msg) => {
        if (msg.type() === 'error' || msg.type() === 'warning') {
            const text = msg.text();
            // Ignore benign browser-level warnings and expected 4xx responses
            // (tests intentionally trigger 401/403/409 — those are not bugs)
            // 5xx errors are NOT filtered — those indicate real server bugs
            if (isFiltered(text)) {
                return;
            }
            messages.push(`[${msg.type()}] ${text}`);
        }
    });

    page.on('pageerror', (err) => {
        const text = `${err.message}\n${err.stack ?? ''}`;
        if (isFiltered(text)) {
            return;
        }
        messages.push(`[pageerror] ${text}`);
    });

    return messages;
}

/**
 * Assert that no console errors or warnings were collected.
 */
export function assertCleanConsole(messages: string[]) {
    expect(messages).toEqual([]);
}

/**
 * Force English language before any page load.
 * Tests assert against English strings; set localStorage before the WASM loads.
 */
export async function setEnglishLanguage(page: Page) {
    await page.addInitScript(() => {
        try {
            localStorage.setItem('spinbike_lang', 'en');
        } catch {
            // ignore — storage not ready
        }
    });
}

/**
 * Simulate running as an installed standalone PWA on iOS (#228): overrides
 * the legacy iOS-Safari-only `navigator.standalone` flag, which is exactly
 * what `platform::is_standalone()` checks FIRST (see `spinbike-ui/src/platform.rs`)
 * — no need to also stub `matchMedia`, since the standalone flag alone
 * already satisfies that check. Must run via `addInitScript` (before the
 * WASM bundle loads) and combined with an iOS `userAgent` context option
 * (e.g. `devices['iPhone 13']`) for `is_ios_standalone()` to be true.
 */
export async function setIosStandalone(page: Page) {
    await page.addInitScript(() => {
        Object.defineProperty(window.navigator, 'standalone', { get: () => true });
    });
}

/**
 * The admin/staff password-login `<form>` on /login, scoped by the ONE
 * attribute only it has: a `type="password"` input. /login also has a
 * SECOND `type="email"` input + submit button — the customer login-link
 * section below this form (#109) — so a bare `input[type="email"]` /
 * `button[type="submit"]` is ambiguous. Scoping by "contains the password
 * input" (rather than `.first()`) stays correct even if the two sections
 * are ever reordered on the page — DOM position isn't the real invariant,
 * "has a password field" is.
 */
export function passwordLoginForm(page: Page) {
    return page.locator('form:has(input[type="password"])');
}

/**
 * Login via the UI: navigate to /login, fill form, submit, wait for redirect.
 */
export async function loginViaUI(page: Page, email: string, password: string) {
    await setEnglishLanguage(page);
    await page.goto('/login');
    await page.waitForSelector('h1.page-title');
    const form = passwordLoginForm(page);
    await form.locator('input[type="email"]').fill(email);
    await form.locator('input[type="password"]').fill(password);
    await form.locator('button[type="submit"]').click();
    // After login, the app redirects to / via location.href
    await page.waitForURL('/', { timeout: 10000 });
}

/**
 * Select the Monthly pass option in the unified card-action service dropdown.
 *
 * The option exposes `data-kind="monthly_pass"` so we don't need to match the
 * visible label, which varies by Lang and includes the price.
 *
 * History: previous incarnation used `selectOption({ label: /regex/ })` (fails
 * — Playwright wants a string) and then a `filter({ hasText: 'Monthly pass' })`
 * lookup (fragile to language switches and renames). The data-kind attribute
 * is the robust handle.
 */
export async function selectMonthlyPass(page: Page): Promise<void> {
    const value = await page
        .locator('[data-testid="charge-service"] option[data-kind="monthly_pass"]')
        .first()
        .getAttribute('value');
    if (!value) throw new Error('Monthly pass option not found (data-kind="monthly_pass")');
    await page.locator('[data-testid="charge-service"]').selectOption(value);
}

/**
 * Login via API and store the token in localStorage so the WASM app picks it up.
 * Returns the raw JWT token so callers can pass it to API requests (e.g. seed endpoints).
 */
export async function loginViaAPI(page: Page, baseURL: string, email: string, password: string): Promise<string> {
    const resp = await fetch(`${baseURL}/api/auth/login`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email, password }),
    });
    if (!resp.ok) {
        throw new Error(`Login failed for ${email}: ${resp.status} ${await resp.text()}`);
    }
    const data = await resp.json();

    // Set English language and auth state before any page loads so the WASM picks it up.
    await setEnglishLanguage(page);
    await page.goto('/');
    await page.evaluate((authData: { token: string; user: { id: number; email: string; name: string; role: string } }) => {
        localStorage.setItem('spinbike_token', authData.token);
        localStorage.setItem('spinbike_user', JSON.stringify(authData.user));
    }, { token: data.token, user: data.user });

    return data.token as string;
}

/**
 * Create a user with a unique name/card_code so it cannot
 * substring-collide with seeded numeric card codes (#39).
 *
 * The 8-char a-z suffix has 26^8 ≈ 2 × 10^11 distinct values — collision
 * with another concurrent test in the same Playwright run is statistically
 * impossible.
 */
export async function createUniqueUser(
    token: string,
    initialCredit: number,
    prefix: string = 'AF',
    email?: string,
): Promise<{ user_id: number; name: string; card_code: string; email?: string }> {
    const BASE_URL = 'http://localhost:8099';
    const suffix = Array.from({ length: 8 }, () =>
        String.fromCharCode(97 + Math.floor(Math.random() * 26)),
    ).join('');
    const cardCode = `${prefix}-${suffix}`;
    const name = `${prefix} ${prefix}${suffix}`;
    const body: Record<string, unknown> = {
        name,
        initial_credit: initialCredit,
        card_code: cardCode,
    };
    if (email) body.email = email;
    const resp = await fetch(`${BASE_URL}/api/users`, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
            Authorization: `Bearer ${token}`,
        },
        body: JSON.stringify(body),
    });
    if (!resp.ok) {
        throw new Error(`createUniqueUser failed: ${resp.status} ${await resp.text()}`);
    }
    const json = await resp.json();
    return { user_id: json.id as number, name, card_code: cardCode, email };
}

/**
 * @deprecated Use createUniqueUser instead.
 * Kept as a thin wrapper for any remaining callers during the migration.
 */
export async function activateUniqueCard(
    token: string,
    initialCredit: number,
    prefix: string = 'AF',
): Promise<{ barcode: string; lastName: string }> {
    const result = await createUniqueUser(token, initialCredit, prefix);
    return { barcode: result.card_code, lastName: result.name.split(' ').slice(1).join('') };
}
