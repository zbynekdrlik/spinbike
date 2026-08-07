import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole } from './helpers';

// #278: `setupConsoleCheck`'s 4xx-derived console-message filter must be
// OPT-IN per test (via `allow4xxFor`, scoped to the endpoint(s) that test
// names) instead of a blanket "any 4xx anywhere" suppression — see the
// design comment on the issue for the full root-cause writeup.
//
// These two tests exercise the scoping mechanism directly, decoupled from
// any specific business flow: this test never logs in, so an in-page fetch
// to the protected `/api/my/balance` endpoint 401s — an UNRELATED 4xx that
// nothing in this test's own flow expects.
test.describe('setupConsoleCheck 4xx scoping (#278)', () => {
    test('an unexpected 4xx with no matching allow-list entry is NOT silently swallowed', async ({ page }) => {
        const messages = setupConsoleCheck(page);
        await page.goto('/login');
        await page.waitForSelector('h1.page-title');

        // No auth token stored anywhere in this test — this 401 is a stand-in
        // for a completely unrelated regression that happens to log a 4xx
        // on the same page. Before #278 the blanket filter hid ANY 4xx here
        // unconditionally, regardless of endpoint, which is exactly what
        // made assertCleanConsole vacuous for a spec like this.
        await page.evaluate(() => fetch('/api/my/balance').catch(() => {}));

        await expect
            .poll(() => messages.some((m) => /the server responded with a status of 4\d\d/.test(m)), {
                timeout: 5000,
            })
            .toBe(true);
    });

    test('a 4xx from an explicitly allow-listed endpoint IS filtered', async ({ page }) => {
        const messages = setupConsoleCheck(page, { allow4xxFor: ['/api/my/balance'] });
        await page.goto('/login');
        await page.waitForSelector('h1.page-title');

        // This is a NEGATIVE assertion (proving a message never arrives), so
        // a bare fixed sleep before checking would be a guess at how long
        // the whole chain takes — exactly the flake risk code review (#278)
        // flagged: on a slow CI runner the console event could still arrive
        // AFTER a short guessed timeout, making the assertion pass for the
        // wrong reason. `waitForResponse` ties the wait to the actual 401
        // landing (the real signal any console message is derived from,
        // same network event); the short trailing buffer only covers the
        // browser's own internal `Log.entryAdded` dispatch, which is not
        // guaranteed to be strictly ordered before the fetch()/Response
        // promise this awaits (see helpers.ts's `msg.location().url` note).
        const responsePromise = page.waitForResponse(
            (r) => r.url().includes('/api/my/balance') && r.status() === 401,
        );
        await page.evaluate(() => fetch('/api/my/balance').catch(() => {}));
        await responsePromise;
        await page.waitForTimeout(200);

        assertCleanConsole(messages);
    });
});
