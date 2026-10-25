import { test, expect, Page } from '@playwright/test';
import {
    setupConsoleCheck,
    assertCleanConsole,
    loginViaAPI,
    seedCustomerAccount,
    createUniqueUser,
} from './helpers';
import {
    BASE_URL,
    ClassOccurrence,
    chargeBooking,
    listClasses,
    listParticipants,
    mondayFirstIndex,
    openStaffSpinningCell,
    staffCancel,
    weekRange,
} from './spinning-fixtures';

/**
 * #389 — a host brings guests to spinning and pays for them from their own
 * credit (owner ROZHODNUTÉ 2026-10-25: a guest is a COUNT on the host's
 * booking, no name). Each guest takes a seat; the 09:00 charge debits the
 * Spinning price per guest from the host; the customer changes the count
 * until 09:00, staff any time (after the charge with an immediate charge /
 * refund).
 *
 * Runs in BOTH Playwright projects (chromium + the real-WebKit iPhone) —
 * customers book on their phones, the CEO runs the staff screen on his.
 */

/** A not-yet-started class (this or next week) the customer may still change, with `seats` free seats. */
async function slotWithFreeSeats(seats: number): Promise<{ slot: ClassOccurrence; week: 0 | 1 }> {
    for (const week of [0, 1] as const) {
        const { from, to } = weekRange(week);
        const slot = (await listClasses(from, to)).find(
            (c) => !c.cancelled && !c.started && c.cancel_open && c.booked + seats <= c.capacity,
        );
        if (slot) return { slot, week };
    }
    throw new Error(`no class with ${seats} free seats in the next two weeks`);
}

async function openScheduleDay(page: Page, week: 0 | 1, date: string): Promise<void> {
    await page.goto('/schedule');
    await page.waitForSelector('h1.page-title');
    await page.waitForFunction(() => !document.querySelector('.spinner'), { timeout: 10000 });
    if (week === 1) {
        await page.locator('[data-testid="week-next"]').click();
        await page.waitForFunction(() => !document.querySelector('.spinner'), { timeout: 10000 });
    }
    await page.locator('.day-picker button').nth(mondayFirstIndex(date)).click();
}

async function myBookingGuests(token: string, bookingId: number): Promise<number> {
    const resp = await fetch(`${BASE_URL}/api/my/bookings`, {
        headers: { Authorization: `Bearer ${token}` },
    });
    expect(resp.status).toBe(200);
    const rows: Array<{ id: number; guests: number }> = await resp.json();
    const row = rows.find((r) => r.id === bookingId);
    if (!row) throw new Error(`booking ${bookingId} not in /api/my/bookings`);
    return row.guests;
}

async function seatsOf(slot: ClassOccurrence): Promise<number> {
    const occ = (await listClasses(slot.date, slot.date)).find((c) => c.template_id === slot.template_id);
    if (!occ) throw new Error(`occurrence ${slot.template_id}/${slot.date} not listed`);
    return occ.booked;
}

test.describe('Spinning guests paid by the host (#389)', () => {
    test('customer adds and removes guests with the "+ host" stepper; seats follow', async ({ page }) => {
        const messages = setupConsoleCheck(page);
        const customer = await seedCustomerAccount(BASE_URL, 'guest');
        const token = await loginViaAPI(page, BASE_URL, customer.email, customer.password);
        const { slot, week } = await slotWithFreeSeats(3);

        const book = await fetch(`${BASE_URL}/api/bookings`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
            body: JSON.stringify({ template_id: slot.template_id, date: slot.date }),
        });
        expect(book.status).toBe(201);
        const bookingId = (await book.json()).id as number;
        try {
            const seatsBefore = await seatsOf(slot);
            await openScheduleDay(page, week, slot.date);

            const key = `${slot.template_id}-${slot.date}`;
            const count = page.locator(`[data-testid="guests-count-${key}"]`);
            const plus = page.locator(`[data-testid="guests-plus-${key}"]`);
            const minus = page.locator(`[data-testid="guests-minus-${key}"]`);
            await expect(count).toHaveText('Guests: 0', { timeout: 10000 });
            await expect(minus).toBeDisabled();

            await plus.click();
            await expect(count).toHaveText('Guests: 1', { timeout: 10000 });
            await plus.click();
            await expect(count).toHaveText('Guests: 2', { timeout: 10000 });
            expect(await myBookingGuests(token, bookingId)).toBe(2);
            expect(await seatsOf(slot)).toBe(seatsBefore + 2);

            await minus.click();
            await expect(count).toHaveText('Guests: 1', { timeout: 10000 });
            expect(await myBookingGuests(token, bookingId)).toBe(1);
            expect(await seatsOf(slot)).toBe(seatsBefore + 1);

            // The home card shows the guests too.
            await page.goto('/my/balance');
            await expect(page.locator(`[data-testid="my-spinning-guests-${bookingId}"]`)).toHaveText(
                'Guests: 1',
                { timeout: 10000 },
            );
            assertCleanConsole(messages);
        } finally {
            await fetch(`${BASE_URL}/api/bookings/${bookingId}`, {
                method: 'DELETE',
                headers: { Authorization: `Bearer ${token}` },
            }).catch(() => {});
        }
    });

    test('staff adjust guests in the table: charged at once after 09:00, refunded on removal', async ({ page }) => {
        const messages = setupConsoleCheck(page);
        const staffToken = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        // Next week: inside the 8-week grid, never started.
        const { from, to } = weekRange(1);
        const slot = (await listClasses(from, to)).find((c) => !c.cancelled && c.booked + 3 <= c.capacity);
        if (!slot) throw new Error(`no class with 3 free seats in ${from}..${to}`);
        const host = await createUniqueUser(staffToken, 50, 'GH');
        const book = await fetch(`${BASE_URL}/api/bookings`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${staffToken}` },
            body: JSON.stringify({ template_id: slot.template_id, date: slot.date, user_id: host.user_id }),
        });
        expect(book.status).toBe(201);
        const bookingId = (await book.json()).id as number;
        const row = async () => {
            const rows = await listParticipants(staffToken, slot.template_id, slot.date);
            const r = rows.find((p) => p.booking_id === bookingId);
            if (!r) throw new Error(`booking ${bookingId} not among participants`);
            return r;
        };
        try {
            await openStaffSpinningCell(page, slot.template_id, slot.date);
            const count = page.locator(`[data-testid="spin-guests-count-${bookingId}"]`);
            const plus = page.locator(`[data-testid="spin-guests-plus-${bookingId}"]`);
            const minus = page.locator(`[data-testid="spin-guests-minus-${bookingId}"]`);
            const credit = page.locator(`[data-testid="spin-credit-${bookingId}"]`);
            await expect(count).toHaveText('0');

            // Before the charge: a guest only takes a seat, no money moves.
            await plus.click();
            await expect(count).toHaveText('1', { timeout: 10000 });
            expect((await row()).guests).toBe(1);
            expect((await row()).credit).toBe(50);

            // The 09:00 charge (test fixture): host + guest from the credit.
            await chargeBooking(staffToken, bookingId);
            const charged = await row();
            expect(charged.charged).toBe(true);
            const price = charged.guest_charged_amount;
            expect(price, 'one guest charged the Spinning price').toBeGreaterThan(0);
            expect(charged.credit).toBeCloseTo(50 - (charged.charged_amount ?? 0) - price, 2);

            // Staff adds a guest after the charge -> charged at once. (Fresh
            // load: the table must show the charged state the fixture wrote.)
            await page.goto('/staff/spinning');
            await openStaffSpinningCell(page, slot.template_id, slot.date);
            await plus.click();
            await expect(count).toHaveText('2', { timeout: 10000 });
            const afterAdd = await row();
            expect(afterAdd.guest_charged_amount).toBeCloseTo(2 * price, 2);
            expect(afterAdd.credit).toBeCloseTo(charged.credit - price, 2);
            await expect(credit).toHaveText(`${afterAdd.credit.toFixed(2)} €`);

            // Staff removes one -> that seat is refunded.
            await minus.click();
            await expect(count).toHaveText('1', { timeout: 10000 });
            const afterRemove = await row();
            expect(afterRemove.guest_charged_amount).toBeCloseTo(price, 2);
            expect(afterRemove.credit).toBeCloseTo(charged.credit, 2);
            await expect(credit).toHaveText(`${afterRemove.credit.toFixed(2)} €`);

            // The occupancy cell counts seats: host + 1 guest.
            const cell = page.locator(`[data-testid="occ-cell-${slot.template_id}-${slot.date}"]`);
            const seats = (await listClasses(slot.date, slot.date)).find(
                (c) => c.template_id === slot.template_id,
            )!.booked;
            await expect(cell).toHaveText(new RegExp(`^${seats}/${slot.capacity}`));
            assertCleanConsole(messages);
        } finally {
            await staffCancel(staffToken, bookingId);
        }
    });
});
