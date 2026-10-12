import { test, expect, Page } from '@playwright/test';
import {
    setupConsoleCheck,
    assertCleanConsole,
    setEnglishLanguage,
    loginViaAPI,
    uniqueLetterSuffix,
} from './helpers';

const BASE_URL = 'http://localhost:8099';

/**
 * #381: the schedule shows the instructor's NAME (it used to print
 * "Instructor #<id>"), Tuesday/Thursday classes run at 18:30 with Vlada
 * (V30), and the admin can permanently delete an instructor no active class
 * uses — but is refused (localized banner, row kept) for one that still
 * teaches.
 */

async function openScheduleDay(page: Page, dayIdx: number) {
    await page.goto('/');
    await page.waitForSelector('h1.page-title');
    await page.waitForFunction(() => !document.querySelector('.spinner'), { timeout: 10000 });
    await page.locator('.day-picker button').nth(dayIdx).click();
}

test.describe('Schedule instructors + Tue/Thu 18:30 (#381)', () => {
    test.beforeEach(async ({ page }) => {
        await setEnglishLanguage(page);
    });

    test('Tuesday and Thursday show the 18:30 class with Vlada, Monday 18:00 with Stopa — names, not ids', async ({ page }) => {
        const messages = setupConsoleCheck(page);

        for (const [dayIdx, time, name] of [
            [1, '18:30', 'Vlada'],
            [3, '18:30', 'Vlada'],
            [0, '18:00', 'Stopa'],
        ] as const) {
            await openScheduleDay(page, dayIdx);
            const row = page.locator('.list-row', {
                has: page.locator('.list-row__title', { hasText: time }),
            });
            await expect(row.first(), `day ${dayIdx}: ${time} class`).toBeVisible({ timeout: 10000 });
            await expect(row.first().locator('[data-testid="class-instructor"]')).toHaveText(
                `Instructor: ${name}`,
            );
            // No card anywhere on the day still prints a raw instructor id.
            await expect(page.locator('.list-row', { hasText: /Instructor #\d/ })).toHaveCount(0);
        }

        assertCleanConsole(messages);
    });
});

test.describe('Admin instructor delete (#381)', () => {
    test('admin deletes an unused instructor; an instructor teaching an active class is refused', async ({ page }) => {
        // The refused delete is a deliberate 409 on exactly this endpoint.
        const messages = setupConsoleCheck(page, { allow4xxFor: ['/api/admin/instructors/1'] });
        const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        const name = `Docasny ${uniqueLetterSuffix()}`;
        const created = await fetch(`${BASE_URL}/api/admin/instructors`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
            body: JSON.stringify({ name }),
        });
        expect(created.status).toBe(201);
        const { id } = await created.json();

        page.on('dialog', (d) => d.accept());
        await page.goto('/admin');
        await page.waitForSelector('h1.page-title');
        await page.locator('[data-testid="admin-tab-instructors"]').click();
        await page.waitForFunction(() => !document.querySelector('.spinner'), { timeout: 10000 });

        // Unused instructor: deleted, row disappears, really gone server-side.
        await expect(page.locator(`[data-testid="instructor-row-${id}"]`)).toContainText(name);
        await page.locator(`[data-testid="instructor-delete-${id}"]`).click();
        await expect(page.locator(`[data-testid="instructor-row-${id}"]`)).toHaveCount(0, { timeout: 10000 });
        const list = await fetch(`${BASE_URL}/api/admin/instructors`, {
            headers: { Authorization: `Bearer ${token}` },
        });
        const rows: Array<{ id: number }> = await list.json();
        expect(rows.some((r) => r.id === id)).toBe(false);

        // Instructor 1 (Stopa) teaches the active Monday/Wednesday classes.
        await page.locator('[data-testid="instructor-delete-1"]').click();
        await expect(page.locator('.alert', { hasText: 'still teaches an active class' })).toBeVisible({
            timeout: 10000,
        });
        await expect(page.locator('[data-testid="instructor-row-1"]')).toContainText('Stopa');

        assertCleanConsole(messages);
    });
});
