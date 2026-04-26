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
 * Select the "Monthly pass" option in the unified card-action service dropdown.
 *
 * Playwright's `selectOption({ label: /regex/ })` does NOT accept regex — it
 * fails with "expected string, got object". The option label is built as
 * `${name} (${default_price:.2} €)` so the price varies between environments;
 * looking up the option by visible text and selecting by its `value` attribute
 * is the robust path.
 */
export async function selectMonthlyPass(page: Page): Promise<void> {
    const value = await page
        .locator('[data-testid="charge-service"] option')
        .filter({ hasText: 'Monthly pass' })
        .first()
        .getAttribute('value');
    if (!value) throw new Error('Monthly pass option not found in [data-testid="charge-service"]');
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
