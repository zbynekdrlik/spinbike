import { test, expect } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, setEnglishLanguage } from './helpers';

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

            const classCards = page.locator('.class-card');
            const count = await classCards.count();
            if (count > 0) {
                foundClass = true;
                // Verify class card has expected structure
                const firstCard = classCards.first();
                await expect(firstCard.locator('.class-time')).toBeVisible();
                await expect(firstCard.locator('.class-spots')).toBeVisible();
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

        // Find a day with classes
        const dayPicker = page.locator('.day-picker');
        const dayButtons = dayPicker.locator('button');

        for (let i = 0; i < 5; i++) {
            await dayButtons.nth(i).click();
            await page.waitForTimeout(300);
            const classCards = page.locator('.class-card');
            if ((await classCards.count()) > 0) {
                // Should have "Login to book" link
                const loginLink = page.locator('a[href="/login"]', { hasText: 'Login to book' });
                await expect(loginLink.first()).toBeVisible();
                break;
            }
        }

        assertCleanConsole(consoleMessages);
    });

    test('authenticated user can book and cancel a class via API and verify in UI', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        // Login via API
        const loginResp = await fetch(`${BASE_URL}/api/auth/login`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ email: 'customer@test.com', password: 'password123' }),
        });
        const loginData = await loginResp.json();
        const token = loginData.token;

        // Determine a date that has a class (today's weekday or next weekday with a template)
        const now = new Date();
        const dayOfWeek = now.getDay(); // 0=Sun, 1=Mon..6=Sat
        // Templates exist for Mon(0), Tue(1), Wed(2), Thu(3), Fri(4) — these are 0-indexed from Mon
        // JS day: Mon=1, Tue=2, Wed=3, Thu=4, Fri=5
        // Pick a weekday that has a template
        const templateDays = [1, 2, 3, 4, 5]; // JS days Mon-Fri
        let targetDay = dayOfWeek;
        if (!templateDays.includes(targetDay)) {
            targetDay = 1; // fallback to Monday
        }
        // Calculate date for this weekday in the current week
        const diff = targetDay - dayOfWeek;
        const targetDate = new Date(now);
        targetDate.setDate(now.getDate() + diff);
        const dateStr = targetDate.toISOString().split('T')[0];

        // Get class list to find template_id
        const from = new Date(now);
        from.setDate(now.getDate() - ((dayOfWeek + 6) % 7)); // Monday of this week
        const to = new Date(from);
        to.setDate(from.getDate() + 6);
        const fromStr = from.toISOString().split('T')[0];
        const toStr = to.toISOString().split('T')[0];

        const classesResp = await fetch(
            `${BASE_URL}/api/classes?from=${fromStr}&to=${toStr}`,
            { headers: { Authorization: `Bearer ${token}` } }
        );
        const classes = await classesResp.json();
        const targetClass = classes.find((c: any) => c.date === dateStr && !c.cancelled && !c.user_booked);

        expect(targetClass).toBeTruthy();

        // Book the class via API
        const bookResp = await fetch(`${BASE_URL}/api/bookings`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
            body: JSON.stringify({ template_id: targetClass.template_id, date: targetClass.date }),
        });
        expect(bookResp.status).toBe(201);
        const bookingData = await bookResp.json();
        const bookingId = bookingData.id;

        // Now verify in the UI that the booking shows as BOOKED
        // Set auth in localStorage for the WASM app
        await page.goto('/');
        await page.evaluate((authData: any) => {
            localStorage.setItem('spinbike_token', authData.token);
            localStorage.setItem('spinbike_user', JSON.stringify(authData.user));
        }, { token: loginData.token, user: loginData.user });

        // Reload to pick up the auth state
        await page.reload();
        await page.waitForSelector('h1.page-title');
        await page.waitForFunction(() => {
            return !document.querySelector('.spinner');
        }, { timeout: 10000 });

        // Click the day that has the booked class
        // targetDay is JS day (1=Mon..5=Fri), day picker buttons are indexed 0=Mon..4=Fri
        const dayIdx = targetDay - 1; // 0-indexed Mon=0
        const dayPicker = page.locator('.day-picker');
        await dayPicker.locator('button').nth(dayIdx).click();
        await page.waitForTimeout(500);

        // Verify BOOKED badge is visible
        const bookedBadge = page.locator('.badge-booked');
        await expect(bookedBadge.first()).toBeVisible({ timeout: 10000 });
        await expect(bookedBadge.first()).toContainText('BOOKED');

        // Cancel the booking via API
        const cancelResp = await fetch(`${BASE_URL}/api/bookings/${bookingId}`, {
            method: 'DELETE',
            headers: { Authorization: `Bearer ${token}` },
        });
        expect(cancelResp.status).toBe(204);

        // Reload and verify the class is bookable again
        await page.reload();
        await page.waitForSelector('h1.page-title');
        await page.waitForFunction(() => {
            return !document.querySelector('.spinner');
        }, { timeout: 10000 });
        await dayPicker.locator('button').nth(dayIdx).click();
        await page.waitForTimeout(500);

        // BOOK button should be visible again
        const bookButton = page.locator('.class-card .class-action button', { hasText: 'BOOK' });
        await expect(bookButton.first()).toBeVisible({ timeout: 10000 });

        assertCleanConsole(consoleMessages);
    });
});
