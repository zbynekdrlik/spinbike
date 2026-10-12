import { test, expect } from '@playwright/test';
import {
    setupConsoleCheck,
    assertCleanConsole,
    setEnglishLanguage,
    seedCustomerAccount,
} from './helpers';
import { cancellableSlotWithinTwoWeeks, mondayFirstIndex } from './spinning-fixtures';

const BASE_URL = 'http://localhost:8099';

test.describe('Schedule and booking', () => {
    test.beforeEach(async ({ page }) => {
        await setEnglishLanguage(page);
    });

    test('public schedule page loads with heading and day picker', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await page.goto('/');
        await page.waitForSelector('h1.page-title');
        expect(await page.textContent('h1.page-title')).toBe('Class Schedule');

        // Day picker should be visible (it has day buttons)
        const dayPicker = page.locator('.day-picker');
        await expect(dayPicker).toBeVisible();

        // Should have 7 day buttons
        const dayButtons = dayPicker.locator('button');
        await expect(dayButtons).toHaveCount(7);

        assertCleanConsole(consoleMessages);
    });

    test('schedule shows class cards after loading', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await page.goto('/');
        await page.waitForSelector('h1.page-title');

        // Wait for loading to finish (spinner disappears)
        await page.waitForFunction(() => {
            return !document.querySelector('.spinner');
        }, { timeout: 10000 });

        // Click through each day to find one with classes
        // Templates exist for Mon(0), Tue(1), Wed(2), Thu(3), Fri(4)
        const dayPicker = page.locator('.day-picker');
        const dayButtons = dayPicker.locator('button');

        let foundClass = false;
        for (let i = 0; i < 5; i++) {
            await dayButtons.nth(i).click();
            // Wait a moment for reactivity
            await page.waitForTimeout(300);

            const classCards = page.locator('.list-row');
            const count = await classCards.count();
            if (count > 0) {
                foundClass = true;
                // Verify class card has expected structure
                const firstCard = classCards.first();
                await expect(firstCard.locator('.list-row__title')).toBeVisible();
                await expect(firstCard.locator('.list-row__sub').nth(1)).toBeVisible();
                break;
            }
        }
        expect(foundClass).toBe(true);

        assertCleanConsole(consoleMessages);
    });

    test('unauthenticated user sees "Login to book" on class cards', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        await page.goto('/');
        await page.waitForSelector('h1.page-title');

        // Wait for loading to finish
        await page.waitForFunction(() => {
            return !document.querySelector('.spinner');
        }, { timeout: 10000 });

        // #382 R4: a class that already started shows "Started" instead of
        // a booking link, so look at NEXT week (all of it is in the future,
        // whatever day the suite runs on). Its Monday has a class.
        await page.locator('[data-testid="week-next"]').click();
        await page.waitForFunction(() => !document.querySelector('.spinner'), { timeout: 10000 });
        await page.locator('.day-picker button').nth(0).click();
        const loginLink = page.locator('a[href="/login"]', { hasText: 'Login to book' });
        await expect(loginLink.first()).toBeVisible({ timeout: 10000 });

        assertCleanConsole(consoleMessages);
    });

    test('authenticated user can book and cancel a class via API and verify in UI', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        // Seed a throwaway customer instead of booking on the SHARED
        // `customer@test.com` (#348). The cancel step sits behind five later
        // assertions, so any earlier failure used to leave a live booking on
        // the shared account -- and the `!c.user_booked` filter below then
        // found nothing on EVERY subsequent run against that database,
        // failing deterministically forever. CI never saw it (fresh DB per
        // run); a local DB survives between runs. A fresh account cannot
        // carry a booking in from a previous failure.
        const customer = await seedCustomerAccount(BASE_URL, 'sched');
        const loginResp = await fetch(`${BASE_URL}/api/auth/login`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ email: customer.email, password: customer.password }),
        });
        const loginData = await loginResp.json();
        const token = loginData.token;

        // #382: pick a class this or next week that has not started and a
        // customer may still cancel (before 09:00 on its day) — the server's
        // own `started` / `cancel_open` flags decide, never local date math.
        const { slot: targetClass, week } = await cancellableSlotWithinTwoWeeks();
        const dayIdx = mondayFirstIndex(targetClass.date);

        const classesResp = await fetch(
            `${BASE_URL}/api/classes?from=${targetClass.date}&to=${targetClass.date}`,
            { headers: { Authorization: `Bearer ${token}` } }
        );
        const classes = await classesResp.json();
        // Isolation guard: a freshly seeded account must own no bookings at
        // all (#348) -- fail here, where the message says so.
        const preBooked = classes.filter((c: any) => c.user_booked);
        expect(
            preBooked,
            'freshly seeded customer already owns bookings -- account is not isolated'
        ).toEqual([]);

        // Book the class via API
        const bookResp = await fetch(`${BASE_URL}/api/bookings`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
            body: JSON.stringify({ template_id: targetClass.template_id, date: targetClass.date }),
        });
        expect(bookResp.status).toBe(201);
        const bookingData = await bookResp.json();
        const bookingId = bookingData.id;
        let released = false;

        // Always give the seat back, even when an assertion below throws.
        // A class has a capacity and a full one answers 409 ClassFull, so
        // abandoned bookings accumulating on a long-lived local database
        // would eventually break this test a second way (#348).
        try {
            // Now verify in the UI that the booking shows as BOOKED
            // Set auth in localStorage for the WASM app
            await page.goto('/');
            await page.evaluate((authData: any) => {
                localStorage.setItem('spinbike_token', authData.token);
                localStorage.setItem('spinbike_user', JSON.stringify(authData.user));
            }, { token: loginData.token, user: loginData.user });

            // Authenticated customers now land on /my/balance from `/` (router
            // redirect for the door self-entry feature, #92). Navigate directly
            // to /schedule for the booking-verification UI flow.
            await page.goto('/schedule');
            await page.waitForSelector('h1.page-title');
            await page.waitForFunction(() => {
                return !document.querySelector('.spinner');
            }, { timeout: 10000 });

            // Show the booked class's week + day.
            if (week === 1) {
                await page.locator('[data-testid="week-next"]').click();
                await page.waitForFunction(() => !document.querySelector('.spinner'), { timeout: 10000 });
            }
            const dayPicker = page.locator('.day-picker');
            await dayPicker.locator('button').nth(dayIdx).click();

            // Verify CANCEL button is visible (booked + still cancellable)
            const cancelBtn = page.locator(
                `[data-testid="cancel-${targetClass.template_id}-${targetClass.date}"]`
            );
            await expect(cancelBtn).toBeVisible({ timeout: 10000 });

            // Cancel the booking via API
            const cancelResp = await fetch(`${BASE_URL}/api/bookings/${bookingId}`, {
                method: 'DELETE',
                headers: { Authorization: `Bearer ${token}` },
            });
            expect(cancelResp.status).toBe(204);
            released = true;

            // Reload and verify the class is bookable again
            await page.reload();
            await page.waitForSelector('h1.page-title');
            await page.waitForFunction(() => {
                return !document.querySelector('.spinner');
            }, { timeout: 10000 });
            if (week === 1) {
                await page.locator('[data-testid="week-next"]').click();
                await page.waitForFunction(() => !document.querySelector('.spinner'), { timeout: 10000 });
            }
            await dayPicker.locator('button').nth(dayIdx).click();

            // BOOK button should be visible again
            const bookButton = page.locator(
                `[data-testid="book-${targetClass.template_id}-${targetClass.date}"]`
            );
            await expect(bookButton).toBeVisible({ timeout: 10000 });

            assertCleanConsole(consoleMessages);
        } finally {
            if (!released) {
                await fetch(`${BASE_URL}/api/bookings/${bookingId}`, {
                    method: 'DELETE',
                    headers: { Authorization: `Bearer ${token}` },
                }).catch(() => {});
            }
        }
    });
});
