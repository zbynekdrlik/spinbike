import { test, expect } from '@playwright/test';
import {
    setupConsoleCheck,
    assertCleanConsole,
    loginViaAPI,
    createUniqueUser,
    contrastOf,
} from './helpers';
import {
    BASE_URL,
    ClassOccurrence,
    apiLogin,
    bookableSlotThisWeek,
    listClasses,
    listParticipants,
    openStaffSpinningCell,
    seedBooking,
    staffCancel,
    weekRange,
} from './spinning-fixtures';

/**
 * The staff class participants, since #385 a TABLE on /staff/spinning (the
 * coloured `.participant-chip` tags are gone — owner: "tabulky, nie stitky").
 *
 * - #379: the participant's name and the row controls must reach WCAG AA
 *   (>= 4.5:1) in BOTH colour schemes — the original chip was ~1.9:1.
 * - #382 R8: a recurring booking is marked ("natrvalo" / "permanent").
 * - #378: the "add person" picker marks a customer already in the class
 *   instead of offering a booking the server would refuse with 409.
 */

/** Any non-cancelled, non-full class of the current gym-local week. */
async function anyClassThisWeek(): Promise<ClassOccurrence> {
    const { from, to } = weekRange(0);
    const slot = (await listClasses(from, to)).find((c) => !c.cancelled && c.booked < c.capacity);
    if (!slot) throw new Error(`no class in ${from}..${to}`);
    return slot;
}

test.describe('Staff participants table contrast (#379, #385)', () => {
    test('name, kind and row actions reach 4.5:1 contrast in dark and light themes', async ({ page }) => {
        const messages = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        // #382 R8: a RECURRING booking, so the kind cell says "permanent" and
        // the row carries the "End permanent" control too. Seeded through the
        // test fixture: the current week's class may already be in the past.
        const slot = await anyClassThisWeek();
        const user = await createUniqueUser(token, 0, 'PC');
        const bookingId = await seedBooking(token, slot.template_id, slot.date, user.user_id, 'persistent');
        try {
            for (const scheme of ['dark', 'light'] as const) {
                await page.emulateMedia({ colorScheme: scheme });
                await page.goto('/staff/spinning');
                await openStaffSpinningCell(page, slot.template_id, slot.date);

                const name = page.locator(`[data-testid="spin-name-${bookingId}"]`);
                await expect(name, `${scheme}: row for ${user.name}`).toHaveText(user.name);
                expect(await contrastOf(name), `${scheme}: name contrast`).toBeGreaterThanOrEqual(4.5);

                const fontPx = await name.evaluate((el) => parseFloat(getComputedStyle(el).fontSize));
                expect(fontPx, `${scheme}: name font size`).toBeGreaterThanOrEqual(14);

                const kind = page.locator(`[data-testid="spin-kind-${bookingId}"]`);
                await expect(kind).toHaveText('permanent');
                expect(await contrastOf(kind), `${scheme}: kind contrast`).toBeGreaterThanOrEqual(4.5);

                for (const id of [`spin-unbook-${bookingId}`, `spin-end-persistent-${bookingId}`]) {
                    const btn = page.locator(`[data-testid="${id}"]`);
                    await expect(btn).toBeVisible();
                    expect(await contrastOf(btn), `${scheme}: ${id} contrast`).toBeGreaterThanOrEqual(4.5);
                }
            }
            assertCleanConsole(messages);
        } finally {
            await staffCancel(token, bookingId);
        }
    });
});

test.describe('Staff add-person picker vs. customers already in the class (#378)', () => {
    test('a booked customer is shown as already booked and cannot be picked; a free one books normally', async ({ page }) => {
        const messages = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        // The one-off booking goes through the real route, which (#382 R4)
        // refuses a class that already started — use one that has not.
        const adminToken = await apiLogin('admin@test.com', 'admin123');
        const { slot, cleanup } = await bookableSlotThisWeek(adminToken);
        const booked = await createUniqueUser(token, 0, 'PB');
        const bookedId = await seedBooking(token, slot.template_id, slot.date, booked.user_id);
        const other = await createUniqueUser(token, 0, 'PW');
        let otherBookingId: number | undefined;
        try {
            await openStaffSpinningCell(page, slot.template_id, slot.date);
            await expect(page.locator(`[data-testid="spin-name-${bookedId}"]`)).toHaveText(booked.name);

            const add = page.locator('[data-testid="spin-add"]');
            // Already-booked customer: marked, no booking buttons — no 409 round trip.
            await add.locator('[data-testid="spin-add-search"]').fill(booked.card_code);
            const bookedHit = add.locator(`[data-testid="spin-add-hit-${booked.user_id}"]`);
            await expect(bookedHit).toBeVisible({ timeout: 5000 });
            await expect(bookedHit.locator(`[data-testid="spin-add-already-${booked.user_id}"]`)).toHaveText(
                'already booked',
            );
            await expect(bookedHit.locator('button')).toHaveCount(0);

            // A customer not yet in the class books normally ("This class only").
            await add.locator('[data-testid="spin-add-search"]').fill(other.card_code);
            const otherHit = add.locator(`[data-testid="spin-add-hit-${other.user_id}"]`);
            await expect(otherHit).toBeVisible({ timeout: 5000 });
            await otherHit.locator(`[data-testid="spin-add-once-${other.user_id}"]`).click();
            await expect(
                page.locator('[data-testid="spin-detail"] [data-testid^="spin-row-"]', { hasText: other.name }),
            ).toHaveCount(1, { timeout: 10000 });

            // Backend effect: the second customer really holds an active booking.
            const participants = await listParticipants(token, slot.template_id, slot.date);
            const mine = participants.filter((p) => p.user_id === other.user_id);
            expect(mine).toHaveLength(1);
            otherBookingId = mine[0].booking_id;

            assertCleanConsole(messages);
        } finally {
            await staffCancel(token, bookedId);
            if (otherBookingId !== undefined) await staffCancel(token, otherBookingId);
            await cleanup();
        }
    });
});
