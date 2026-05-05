import { Page, expect } from '@playwright/test';

/**
 * Set up console error/warning collection on a page.
 * Returns an array that accumulates messages during the test.
 */
export function setupConsoleCheck(page: Page): string[] {
    const messages: string[] = [];
    page.on('console', (msg) => {
        if (msg.type() === 'error' || msg.type() === 'warning') {
            const text = msg.text();
            // Ignore benign browser-level warnings and expected 4xx responses
            // (tests intentionally trigger 401/403/409 — those are not bugs)
            // 5xx errors are NOT filtered — those indicate real server bugs
            if (
                text.includes('SharedArrayBuffer') ||
                text.includes('wasm') ||
                text.includes('integrity') ||
                text.includes('subresource integrity') ||
                text.includes('crbug.com') ||
                /the server responded with a status of 4\d\d/.test(text)
            ) {
                return;
            }
            messages.push(`[${msg.type()}] ${text}`);
        }
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
 * Login via the UI: navigate to /login, fill form, submit, wait for redirect.
 */
export async function loginViaUI(page: Page, email: string, password: string) {
    await setEnglishLanguage(page);
    await page.goto('/login');
    await page.waitForSelector('h1.page-title');
    await page.fill('input[type="email"]', email);
    await page.fill('input[type="password"]', password);
    await page.click('button[type="submit"]');
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
): Promise<{ user_id: number; name: string; card_code: string }> {
    const BASE_URL = 'http://localhost:8099';
    const suffix = Array.from({ length: 8 }, () =>
        String.fromCharCode(97 + Math.floor(Math.random() * 26)),
    ).join('');
    const cardCode = `${prefix}-${suffix}`;
    const name = `${prefix} ${prefix}${suffix}`;
    const resp = await fetch(`${BASE_URL}/api/users`, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
            Authorization: `Bearer ${token}`,
        },
        body: JSON.stringify({
            name,
            initial_credit: initialCredit,
            card_code: cardCode,
        }),
    });
    if (!resp.ok) {
        throw new Error(`createUniqueUser failed: ${resp.status} ${await resp.text()}`);
    }
    const json = await resp.json();
    return { user_id: json.id as number, name, card_code: cardCode };
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
