import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole } from './helpers';

// Customer bookings list on /my/bookings (#146). The owner complained the row
// showed "Class #<internal template id> — <raw ISO date>" — meaningless to a
// customer. This asserts the row now shows a localized date + the class
// start time + the instructor's name instead.

const BASE_URL = 'http://localhost:8099';

function randSuffix(): string {
    return Array.from({ length: 8 }, () =>
        String.fromCharCode(97 + Math.floor(Math.random() * 26)),
    ).join('');
}

/** A Monday at least 7 days out, matching the weekday=0 slot V6 seeds. */
function futureMonday(): string {
    const d = new Date();
    d.setDate(d.getDate() + 7);
    while (d.getDay() !== 1) {
        d.setDate(d.getDate() + 1);
    }
    return d.toISOString().split('T')[0];
}

test.describe('Customer bookings on /my/bookings (#146)', () => {
    test('row shows formatted date/time + instructor, not "Class #<id> — <ISO>"', async ({ page, baseURL }) => {
        const messages = setupConsoleCheck(page);
        const suffix = randSuffix();
        const email = `MB-${suffix}@test.local`;
        const password = `Pw-${suffix}`;

        const seedResp = await fetch(`${BASE_URL}/api/test/seed-account`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ email, password, name: `MB ${suffix}`, role: 'customer' }),
        });
        if (!seedResp.ok) throw new Error(`seed-account failed: ${seedResp.status} ${await seedResp.text()}`);

        // V6 migration seeds a Monday 18:00 template taught by "Stevo" — find
        // it via the public schedule endpoint rather than hardcoding a
        // template_id.
        const date = futureMonday();
        const occResp = await fetch(`${BASE_URL}/api/classes?from=${date}&to=${date}`);
        if (!occResp.ok) throw new Error(`list classes failed: ${occResp.status} ${await occResp.text()}`);
        const occurrences: Array<{ template_id: number; start_time: string }> = await occResp.json();
        const slot = occurrences.find((o) => o.start_time === '18:00');
        if (!slot) throw new Error(`no 18:00 Monday occurrence found on ${date}: ${JSON.stringify(occurrences)}`);

        const custToken = await loginViaAPI(page, baseURL!, email, password);
        const bookResp = await fetch(`${BASE_URL}/api/bookings`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${custToken}` },
            body: JSON.stringify({ template_id: slot.template_id, date }),
        });
        if (!bookResp.ok) throw new Error(`booking failed: ${bookResp.status} ${await bookResp.text()}`);

        await page.goto('/my/bookings');
        const row = page.locator('.list-row').first();
        await expect(row).toBeVisible({ timeout: 8000 });
        const text = (await row.textContent()) ?? '';

        // The old bug: raw internal template id + unformatted ISO date.
        expect(text).not.toContain('Class #');
        expect(text).not.toMatch(/\d{4}-\d{2}-\d{2}/);

        // New: the class start time + the instructor's name are shown.
        expect(text).toContain('18:00');
        expect(text).toContain('Stevo');

        assertCleanConsole(messages);
    });
});
