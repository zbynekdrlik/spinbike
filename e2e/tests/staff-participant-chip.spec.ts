import { test, expect, Page, Locator } from '@playwright/test';
import {
    setupConsoleCheck,
    assertCleanConsole,
    loginViaAPI,
    createUniqueUser,
} from './helpers';
import {
    ClassOccurrence,
    apiLogin,
    bookableSlotThisWeek,
    listClasses,
    seedBooking,
    weekRange,
} from './spinning-fixtures';

const BASE_URL = 'http://localhost:8099';

/**
 * #379: participant names on the staff class roster (/staff/classes) used to
 * render as a `.badge` with an inline `background:#e0e7ff` while the text
 * colour came from the theme (`var(--text-muted)`, #a8acb5 in the default
 * dark theme) — ~1.9:1 contrast, unreadable. The chip must reach WCAG AA
 * (>= 4.5:1) for both the name and the remove (✕) control, in BOTH colour
 * schemes, at a readable size.
 */

interface Booked {
    bookingId: number;
    name: string;
    userId: number;
    cardCode: string;
    templateId: number;
    date: string;
}

/**
 * Put a fresh, uniquely named customer into `slot` (a class of the roster's
 * current week). #382: the booking is seeded through the test-only fixture,
 * because the real POST /api/bookings refuses a class that already started
 * and the current week's first class is often in the past.
 */
async function bookFreshCustomer(token: string, slot: ClassOccurrence): Promise<Booked> {
    const user = await createUniqueUser(token, 0, 'PC');
    const id = await seedBooking(token, slot.template_id, slot.date, user.user_id);
    return {
        bookingId: id,
        name: user.name,
        userId: user.user_id,
        cardCode: user.card_code,
        templateId: slot.template_id,
        date: slot.date,
    };
}

/** Any non-cancelled, non-full class of the current gym-local week. */
async function anyClassThisWeek(): Promise<ClassOccurrence> {
    const { from, to } = weekRange(0);
    const slot = (await listClasses(from, to)).find((c) => !c.cancelled && c.booked < c.capacity);
    if (!slot) throw new Error(`no class in ${from}..${to}`);
    return slot;
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
        const booked = await bookFreshCustomer(token, await anyClassThisWeek());
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

test.describe('Staff walk-in picker vs. customers already in the class (#378)', () => {
    test('a booked customer is shown as already booked and cannot be picked; a free one books normally', async ({ page }) => {
        const messages = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        // The walk-in books through the real route, which (#382 R4) refuses
        // a class that already started — use one that has not.
        const adminToken = await apiLogin('admin@test.com', 'admin123');
        const { slot, cleanup } = await bookableSlotThisWeek(adminToken);
        const booked = await bookFreshCustomer(token, slot);
        const other = await createUniqueUser(token, 0, 'PW');
        let otherBookingId: number | undefined;
        try {
            await openRoster(page, 'dark');
            const card = page.locator(`[data-testid="roster-class-${booked.templateId}-${booked.date}"]`);
            await expect(card).toBeVisible({ timeout: 10000 });
            await expect(card.locator('.participant-chip', { hasText: booked.name })).toBeVisible();

            await card.locator('[data-testid="add-walkin"]').click();
            const form = card.locator('[data-testid="walkin-form"]');
            await expect(form).toBeVisible();

            // Already-booked customer: marked, disabled — no 409 round trip.
            await form.locator('[data-testid="walkin-search"]').fill(booked.cardCode);
            const bookedRow = form.locator(`[data-testid="walkin-pick-${booked.userId}"]`);
            await expect(bookedRow).toBeVisible({ timeout: 5000 });
            await expect(bookedRow).toHaveAttribute('data-already-booked', 'true');
            await expect(bookedRow.locator('button')).toBeDisabled();
            await expect(bookedRow.locator('button')).toContainText('already booked');

            // A customer not yet in the class books normally.
            await form.locator('[data-testid="walkin-search"]').fill(other.card_code);
            const otherRow = form.locator(`[data-testid="walkin-pick-${other.user_id}"]`);
            await expect(otherRow).toBeVisible({ timeout: 5000 });
            await expect(otherRow).toHaveAttribute('data-already-booked', 'false');
            await otherRow.locator('button').click();
            await expect(card.locator('.participant-chip', { hasText: other.name })).toBeVisible({ timeout: 10000 });

            // Backend effect: the second customer really holds an active booking.
            const resp = await fetch(
                `${BASE_URL}/api/classes/${booked.templateId}/${booked.date}/participants`,
                { headers: { Authorization: `Bearer ${token}` } },
            );
            expect(resp.status).toBe(200);
            const participants: Array<{ booking_id: number; user_id: number }> = await resp.json();
            const mine = participants.filter((p) => p.user_id === other.user_id);
            expect(mine).toHaveLength(1);
            otherBookingId = mine[0].booking_id;

            assertCleanConsole(messages);
        } finally {
            await cancelBooking(token, booked.bookingId);
            if (otherBookingId !== undefined) await cancelBooking(token, otherBookingId);
            await cleanup();
        }
    });
});
