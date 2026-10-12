import { test, expect, Page, Locator } from '@playwright/test';
import {
    setupConsoleCheck,
    assertCleanConsole,
    loginViaAPI,
    createUniqueUser,
    bratislavaToday,
} from './helpers';

const BASE_URL = 'http://localhost:8099';

/**
 * #379: participant names on the staff class roster (/staff/classes) used to
 * render as a `.badge` with an inline `background:#e0e7ff` while the text
 * colour came from the theme (`var(--text-muted)`, #a8acb5 in the default
 * dark theme) — ~1.9:1 contrast, unreadable. The chip must reach WCAG AA
 * (>= 4.5:1) for both the name and the remove (✕) control, in BOTH colour
 * schemes, at a readable size.
 */

/** Monday..Sunday (YYYY-MM-DD) of the current gym-local week. */
function currentWeekRange(): { from: string; to: string } {
    const [y, m, d] = bratislavaToday().split('-').map(Number);
    const today = new Date(Date.UTC(y, m - 1, d));
    const sinceMonday = (today.getUTCDay() + 6) % 7;
    const monday = new Date(Date.UTC(y, m - 1, d - sinceMonday));
    const sunday = new Date(Date.UTC(y, m - 1, d - sinceMonday + 6));
    return { from: monday.toISOString().slice(0, 10), to: sunday.toISOString().slice(0, 10) };
}

interface Booked {
    bookingId: number;
    name: string;
    userId: number;
    cardCode: string;
    templateId: number;
    date: string;
}

/** Book a fresh, uniquely named customer into a class of the current week. */
async function bookFreshCustomerThisWeek(token: string): Promise<Booked> {
    const user = await createUniqueUser(token, 0, 'PC');
    const { from, to } = currentWeekRange();
    const resp = await fetch(`${BASE_URL}/api/classes?from=${from}&to=${to}`, {
        headers: { Authorization: `Bearer ${token}` },
    });
    if (!resp.ok) throw new Error(`list classes failed: ${resp.status} ${await resp.text()}`);
    const classes: Array<{ template_id: number; date: string; cancelled: boolean; booked: number; capacity: number }> =
        await resp.json();
    const slot = classes.find((c) => !c.cancelled && c.booked < c.capacity);
    if (!slot) throw new Error(`no bookable class in ${from}..${to}: ${JSON.stringify(classes)}`);

    const book = await fetch(`${BASE_URL}/api/bookings`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ template_id: slot.template_id, date: slot.date, user_id: user.user_id }),
    });
    if (book.status !== 201) throw new Error(`booking failed: ${book.status} ${await book.text()}`);
    const { id } = await book.json();
    return {
        bookingId: id as number,
        name: user.name,
        userId: user.user_id,
        cardCode: user.card_code,
        templateId: slot.template_id,
        date: slot.date,
    };
}

async function cancelBooking(token: string, bookingId: number): Promise<void> {
    await fetch(`${BASE_URL}/api/bookings/${bookingId}`, {
        method: 'DELETE',
        headers: { Authorization: `Bearer ${token}` },
    }).catch(() => {});
}

/**
 * WCAG contrast of `fg`'s text colour against the EFFECTIVE background behind
 * it (walks up the ancestors, compositing translucent layers), computed in
 * the page from `getComputedStyle`.
 */
async function contrastOf(fg: Locator): Promise<number> {
    return fg.evaluate((el) => {
        type RGBA = [number, number, number, number];
        const parse = (s: string): RGBA => {
            const nums = (s.match(/[\d.]+/g) ?? []).map(Number);
            if (s.startsWith('color(srgb')) {
                return [nums[0] * 255, nums[1] * 255, nums[2] * 255, nums.length > 3 ? nums[3] : 1];
            }
            return [nums[0], nums[1], nums[2], nums.length > 3 ? nums[3] : 1];
        };
        const over = (top: RGBA, bottom: RGBA): RGBA => {
            const a = top[3];
            return [
                top[0] * a + bottom[0] * (1 - a),
                top[1] * a + bottom[1] * (1 - a),
                top[2] * a + bottom[2] * (1 - a),
                1,
            ];
        };
        // Background layers from `el` upward until an opaque one.
        const layers: RGBA[] = [];
        let node: Element | null = el;
        while (node) {
            const bg = parse(getComputedStyle(node).backgroundColor);
            if (bg[3] > 0) {
                layers.push(bg);
                if (bg[3] >= 1) break;
            }
            node = node.parentElement;
        }
        let bg: RGBA = [255, 255, 255, 1];
        for (let i = layers.length - 1; i >= 0; i--) bg = over(layers[i], bg);
        const fgc = over(parse(getComputedStyle(el).color), bg);
        const lum = (c: RGBA) => {
            const ch = c.slice(0, 3).map((v) => {
                const x = v / 255;
                return x <= 0.03928 ? x / 12.92 : Math.pow((x + 0.055) / 1.055, 2.4);
            });
            return 0.2126 * ch[0] + 0.7152 * ch[1] + 0.0722 * ch[2];
        };
        const [hi, lo] = [lum(fgc), lum(bg)].sort((a, b) => b - a);
        return (hi + 0.05) / (lo + 0.05);
    });
}

async function openRoster(page: Page, scheme: 'dark' | 'light') {
    await page.emulateMedia({ colorScheme: scheme });
    await page.goto('/staff/classes');
    await page.waitForSelector('h1.page-title');
    await page.waitForFunction(() => !document.querySelector('.spinner'), { timeout: 10000 });
}

test.describe('Staff class roster participant chips (#379)', () => {
    test('participant name and remove control reach 4.5:1 contrast in dark and light themes', async ({ page }) => {
        const messages = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const booked = await bookFreshCustomerThisWeek(token);
        try {
            for (const scheme of ['dark', 'light'] as const) {
                await openRoster(page, scheme);
                const chip = page.locator('.participants-list span', { hasText: booked.name }).first();
                await expect(chip, `${scheme}: chip for ${booked.name}`).toBeVisible({ timeout: 10000 });

                const nameContrast = await contrastOf(chip);
                expect(nameContrast, `${scheme}: participant name contrast`).toBeGreaterThanOrEqual(4.5);

                const remove = chip.locator('button');
                const removeContrast = await contrastOf(remove);
                expect(removeContrast, `${scheme}: remove (✕) contrast`).toBeGreaterThanOrEqual(4.5);

                const fontPx = await chip.evaluate((el) => parseFloat(getComputedStyle(el).fontSize));
                expect(fontPx, `${scheme}: chip font size`).toBeGreaterThanOrEqual(15);
            }
            assertCleanConsole(messages);
        } finally {
            await cancelBooking(token, booked.bookingId);
        }
    });
});
