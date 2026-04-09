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
            // Ignore only truly benign browser-level warnings
            if (
                text.includes('SharedArrayBuffer') ||
                text.includes('wasm') ||
                text.includes('integrity') ||
                text.includes('subresource integrity') ||
                text.includes('crbug.com')
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
 * Login via the UI: navigate to /login, fill form, submit, wait for redirect.
 */
export async function loginViaUI(page: Page, email: string, password: string) {
    await page.goto('/login');
    await page.waitForSelector('h1.page-title');
    await page.fill('input[type="email"]', email);
    await page.fill('input[type="password"]', password);
    await page.click('button[type="submit"]');
    // After login, the app redirects to / via location.href
    await page.waitForURL('/', { timeout: 10000 });
}

/**
 * Login via API and store the token in localStorage so the WASM app picks it up.
 */
export async function loginViaAPI(page: Page, baseURL: string, email: string, password: string) {
    const resp = await fetch(`${baseURL}/api/auth/login`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email, password }),
    });
    if (!resp.ok) {
        throw new Error(`Login failed for ${email}: ${resp.status} ${await resp.text()}`);
    }
    const data = await resp.json();

    // Set localStorage so the WASM app sees the auth state
    await page.goto('/');
    await page.evaluate((authData: { token: string; user: { id: number; email: string; name: string; role: string } }) => {
        localStorage.setItem('spinbike_token', authData.token);
        localStorage.setItem('spinbike_user', JSON.stringify(authData.user));
    }, { token: data.token, user: data.user });
}
