import { Page, expect } from '@playwright/test';

/**
 * Today's date in Europe/Bratislava, as YYYY-MM-DD — matches the server's
 * `util::today_bratislava()` anchor. `spinbike-server`'s day/range reports
 * (and several other endpoints) bucket transactions by the GYM-LOCAL day,
 * not raw UTC (#251) — so any E2E test that needs "today" to agree with the
 * server's own day-boundary decision MUST use this, never
 * `new Date().toISOString().slice(0, 10)` (a UTC date). The two disagree
 * during the 00:00-02:00 Bratislava-local window (a UTC CI runner can still
 * be on the previous UTC day while Bratislava has already rolled over) —
 * an intermittent, CI-only flake that's hard to reproduce on a non-UTC dev
 * machine (#251 surfaced exactly this way).
 */
export function bratislavaToday(): string {
    return new Date().toLocaleDateString('en-CA', { timeZone: 'Europe/Bratislava' });
}

/**
 * `bratislavaToday()` shifted by `days` CALENDAR days (negative = past).
 * Pure calendar-date arithmetic (via `Date.UTC` on the broken-down Y/M/D,
 * which normalizes month/day overflow) — no timezone ambiguity, unlike
 * adding milliseconds to `Date.now()` and re-deriving a UTC date from that.
 */
export function bratislavaDateOffset(days: number): string {
    const [y, m, d] = bratislavaToday().split('-').map(Number);
    return new Date(Date.UTC(y, m - 1, d + days)).toISOString().slice(0, 10);
}

export interface ConsoleCheckOptions {
    /**
     * URL substrings whose 4xx-derived browser network message
     * ("Failed to load resource: the server responded with a status of
     * 4xx") is the DELIBERATE subject of THIS test and should be treated
     * as benign — e.g. a wrong-password login attempt hitting
     * `/api/auth/login`. Matched via `text.includes(needle)`.
     *
     * Defaults to `[]` — no 4xx message is filtered. Before #278 this
     * filter was a single BLANKET regex applied to every caller, which
     * made `assertCleanConsole` vacuous for any spec whose flow legitimately
     * triggers a 4xx: it silently swallowed 4xx noise from completely
     * UNRELATED endpoints too, so a real regression logging an unexpected
     * 4xx-derived console error would still pass green. Opting in per test,
     * scoped to the exact endpoint(s) that test expects, keeps "clean
     * console" meaning "nothing unexpected happened".
     */
    allow4xxFor?: string[];
}

/**
 * Set up console error/warning collection on a page.
 * Returns an array that accumulates messages during the test.
 *
 * Captures both console.error / console.warning AND uncaught page errors
 * (which is how wasm-bindgen `throw` events surface — they are NOT
 * console events, so listening only to 'console' silently misses every
 * Leptos panic. See #89.)
 */
export function setupConsoleCheck(page: Page, options: ConsoleCheckOptions = {}): string[] {
    const messages: string[] = [];
    const allow4xxFor = options.allow4xxFor ?? [];

    const is4xxNetworkMessage = (text: string): boolean =>
        /the server responded with a status of 4\d\d/.test(text);

    // #278 [green, round 2]: the browser-generated "Failed to load resource:
    // the server responded with a status of 4xx" console message NEVER
    // contains the resource URL in its `text` — matching `allow4xxFor`
    // needles against `text.includes(needle)` (the first #278 fix) can
    // therefore never match, so every intentional 4xx leaked through as an
    // "unexpected" console error (CI run 31197701928, 23 specs). Confirmed
    // empirically (standalone Playwright script against a local HTTP
    // server): Chromium reports these as `Log.entryAdded` browser-level
    // messages, and Playwright surfaces their resource URL via
    // `msg.location().url` — NOT via `msg.text()`. `page.on('pageerror')`
    // (uncaught JS exceptions) has no such resource location, so only the
    // console path takes a location.
    const isFiltered = (text: string, locationUrl?: string): boolean =>
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
        // #278: a 4xx-derived network message is filtered ONLY when the
        // FAILED RESOURCE'S URL (never the message text) matches an
        // endpoint the caller explicitly named as its own deliberate 4xx
        // (see ConsoleCheckOptions.allow4xxFor above) — never blanket.
        (is4xxNetworkMessage(text) &&
            locationUrl !== undefined &&
            allow4xxFor.some((needle) => locationUrl.includes(needle)));

    page.on('console', (msg) => {
        if (msg.type() === 'error' || msg.type() === 'warning') {
            const text = msg.text();
            // Ignore benign browser-level warnings and the caller's own
            // explicitly-allowed 4xx responses (tests intentionally
            // triggering 401/403/409 on a NAMED endpoint — see
            // allow4xxFor above). 5xx errors are NEVER filtered — those
            // indicate real server bugs.
            if (isFiltered(text, msg.location().url)) {
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
 * Random letters-only (a-z) suffix for any E2E fixture identifier
 * (card_code, barcode, name) that must never collide with a numeric
 * substring search.
 *
 * `dashboard.spec.ts`'s staff search box (and any future short-substring
 * search) does an UNANCHORED `search_text LIKE '%query%'` against every
 * user's name+company+card_code in the shared CI E2E database — all ~80
 * spec files run serially (workers: 1) against ONE server + ONE SQLite DB
 * for the whole run, so records accumulate WITHIN a run. A suffix built
 * from `Date.now()` is PURE DIGITS and can substring-collide with a short
 * digit query typed by a completely unrelated spec (#39, closed 2026-05-01
 * via commit f289ac7 — then recurred through `category-revenue.spec.ts`'s
 * own `Date.now()`-based card_code, confirmed live on CI run 31178634087:
 * "CR Reports1786106601001s4hn" outranked "Jana Testova" in a '1001'
 * search). Letters-only makes that collision impossible BY CONSTRUCTION —
 * no digit substring can ever appear in an a-z-only string.
 *
 * 26^len distinct values (len=8 -> ≈2×10^11) also makes a same-run
 * collision between two GENERATED suffixes themselves statistically
 * impossible.
 */
export function uniqueLetterSuffix(len = 8): string {
    return Array.from({ length: len }, () =>
        String.fromCharCode(97 + Math.floor(Math.random() * 26)),
    ).join('');
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
    const suffix = uniqueLetterSuffix();
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
