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
    weekRange,
    listClasses,
    seedBooking,
    chargeBooking,
    staffCancel,
} from './spinning-fixtures';

/**
 * #382 — spinning recurring bookings, customer self-service and the staff
 * cancel-with-refund (spec docs/superpowers/specs/2026-10-12-spinning-recurring-bookings-design.md).
 *
 * - R7: a customer who never spins sees NO spinning card on home; everyone
 *   has a discreet "Spinning" entry in the burger menu (-> /schedule).
 * - R5: a customer subscribes to a weekly slot on /schedule and immediately
 *   sees the materialised bookings at the top of home ("Moje spinningy");
 *   cancelling ONE date removes only that date, the subscription stays.
 * - R3/R8: staff cancel a CHARGED booking from the roster chip (labelled
 *   "Cancel and refund credit") and the customer's credit comes back as a
 *   separate storno ledger row.
 */

interface Slot {
    template_id: number;
    weekday: number;
    start_time: string;
    instructor_name: string | null;
    subscribed: boolean;
}

async function mySlots(token: string): Promise<Slot[]> {
    const resp = await fetch(`${BASE_URL}/api/my/persistent-bookings`, {
        headers: { Authorization: `Bearer ${token}` },
    });
    if (!resp.ok) throw new Error(`my slots failed: ${resp.status} ${await resp.text()}`);
    return resp.json();
}

async function setMySlots(token: string, templateIds: number[]): Promise<void> {
    await fetch(`${BASE_URL}/api/my/persistent-bookings`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ template_ids: templateIds }),
    });
}

async function myBookingIds(token: string): Promise<number[]> {
    const resp = await fetch(`${BASE_URL}/api/my/bookings`, {
        headers: { Authorization: `Bearer ${token}` },
    });
    if (!resp.ok) throw new Error(`my bookings failed: ${resp.status} ${await resp.text()}`);
    const rows: Array<{ id: number }> = await resp.json();
    return rows.map((r) => r.id);
}

/** /my/balance with the balance response actually rendered (not the "—" placeholder). */
async function openHome(page: Page): Promise<void> {
    await page.goto('/my/balance');
    await expect(page.locator('[data-testid="my-balance-credit"] .card-credit__value')).toContainText(
        /\d/,
        { timeout: 10000 },
    );
}

test.describe('Spinning on the customer home + menu (#382)', () => {
    test('a non-spinner sees no spinning card, but the menu has a discreet Spinning entry to the schedule', async ({
        page,
    }) => {
        const messages = setupConsoleCheck(page);
        const customer = await seedCustomerAccount(BASE_URL, 'nonspin');
        await loginViaAPI(page, BASE_URL, customer.email, customer.password);

        await openHome(page);
        await expect(page.locator('[data-testid="my-spinning-card"]')).toHaveCount(0);

        await page.locator('[data-testid="navbar-burger"]').click();
        const entry = page.locator('[data-testid="navbar-menu-sheet"] [data-testid="menu-spinning"]');
        await expect(entry).toBeVisible();
        await expect(entry).toHaveText('Spinning');
        await entry.click();
        await page.waitForURL('**/schedule');
        await expect(page.locator('[data-testid="my-slots"]')).toBeVisible({ timeout: 10000 });

        assertCleanConsole(messages);
    });

    test('subscribing to a weekly slot puts the bookings on top of home; cancelling one date keeps the rest', async ({
        page,
    }) => {
        const messages = setupConsoleCheck(page);
        const customer = await seedCustomerAccount(BASE_URL, 'spinner');
        const token = await loginViaAPI(page, BASE_URL, customer.email, customer.password);
        const monday = (await mySlots(token)).find((s) => s.weekday === 0 && s.start_time === '18:00');
        if (!monday) throw new Error('no Monday 18:00 slot offered');
        expect(monday.subscribed, 'fresh account must not be subscribed (isolation)').toBe(false);

        try {
            // Subscribe through the real UI on the schedule page.
            await page.goto('/schedule');
            const toggle = page.locator(`[data-testid="slot-toggle-${monday.template_id}"]`);
            await expect(toggle).toBeVisible({ timeout: 10000 });
            await expect(toggle).toHaveAttribute('data-subscribed', 'false');
            await toggle.click();
            await expect(toggle).toHaveAttribute('data-subscribed', 'true', { timeout: 10000 });

            // Backend effect: subscribed + bookings materialised immediately.
            expect((await mySlots(token)).find((s) => s.template_id === monday.template_id)?.subscribed).toBe(true);
            const booked = await myBookingIds(token);
            expect(booked.length, 'the next Mondays are booked at once').toBeGreaterThanOrEqual(2);

            // Home: the "Moje spinningy" card is the FIRST thing after the title.
            await openHome(page);
            const card = page.locator('[data-testid="my-spinning-card"]');
            await expect(card).toBeVisible({ timeout: 10000 });
            const cardIsAboveCredit = await page.evaluate(() => {
                const c = document.querySelector('[data-testid="my-spinning-card"]');
                const credit = document.querySelector('[data-testid="my-balance-credit"]');
                return !!c && !!credit && !!(c.compareDocumentPosition(credit) & Node.DOCUMENT_POSITION_FOLLOWING);
            });
            expect(cardIsAboveCredit, 'the spinning card sits above the credit card').toBe(true);
            const rows = card.locator('[data-testid^="my-spinning-row-"]');
            await expect(rows.first()).toBeVisible({ timeout: 10000 });
            await expect(rows.first()).toContainText('18:00');
            await expect(rows.first()).toContainText('Stopa');
            await expect(rows.first()).toContainText('recurring');

            // Cancel ONE date from the home card.
            const cancelBtn = card.locator('[data-testid^="my-spinning-cancel-"]').first();
            await expect(cancelBtn).toBeVisible();
            const cancelledId = Number(
                (await cancelBtn.getAttribute('data-testid'))!.replace('my-spinning-cancel-', ''),
            );
            await cancelBtn.click();
            await expect(card.locator(`[data-testid="my-spinning-row-${cancelledId}"]`)).toHaveCount(0, {
                timeout: 10000,
            });

            // Backend: only that date is gone; the subscription stays active.
            const after = await myBookingIds(token);
            expect(after).not.toContain(cancelledId);
            expect(after.length).toBe(booked.length - 1);
            expect((await mySlots(token)).find((s) => s.template_id === monday.template_id)?.subscribed).toBe(true);

            // Manage link goes to the schedule (slots).
            await card.locator('[data-testid="my-spinning-manage"]').click();
            await page.waitForURL('**/schedule');

            assertCleanConsole(messages);
        } finally {
            // Free the seats on the shared E2E database (#348 pattern).
            await setMySlots(token, []);
        }
    });
});

test.describe('Staff cancel with refund (#382 R3/R8)', () => {
    test('cancelling a charged booking from the roster chip refunds the credit as a separate ledger row', async ({
        page,
    }) => {
        const messages = setupConsoleCheck(page);
        const staffToken = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const customer = await createUniqueUser(staffToken, 10, 'RF');

        // Any class of the roster's current week — staff may cancel at any
        // time, so a class that already took place is fine (and realistic).
        const { from, to } = weekRange(0);
        const slot = (await listClasses(from, to)).find((c) => !c.cancelled && c.booked < c.capacity);
        if (!slot) throw new Error(`no class in ${from}..${to}`);
        const bookingId = await seedBooking(staffToken, slot.template_id, slot.date, customer.user_id);
        let cancelled = false;
        try {
            await chargeBooking(staffToken, bookingId);
            const creditOf = async () => {
                const r = await fetch(`${BASE_URL}/api/users/lookup/${customer.card_code}`, {
                    headers: { Authorization: `Bearer ${staffToken}` },
                });
                return (await r.json()).credit as number;
            };
            const charged = await creditOf();
            expect(charged, 'the binding charge debited the Spinning price').toBeLessThan(10);

            await page.goto('/staff/classes');
            await page.waitForSelector('h1.page-title');
            const roster = page.locator(`[data-testid="roster-class-${slot.template_id}-${slot.date}"]`);
            await expect(roster).toBeVisible({ timeout: 10000 });
            const chip = roster.locator(`[data-testid="participant-chip-${bookingId}"]`);
            await expect(chip).toBeVisible({ timeout: 10000 });
            const remove = chip.locator(`[data-testid="participant-remove-${bookingId}"]`);
            await expect(remove).toHaveAttribute('title', 'Cancel and refund credit');
            await remove.click();
            await expect(chip).toHaveCount(0, { timeout: 10000 });
            cancelled = true;

            expect(await creditOf(), 'credit restored').toBeCloseTo(10, 2);
            const txResp = await fetch(`${BASE_URL}/api/users/${customer.user_id}/transactions`, {
                headers: { Authorization: `Bearer ${staffToken}` },
            });
            const txs: Array<{ amount: number; action: string; service_kind: string | null }> = await txResp.json();
            // The refund carries no service (it is never counted as a visit).
            const refund = txs.filter((t) => t.action === 'storno');
            expect(refund, 'exactly one separate refund row').toHaveLength(1);
            expect(refund[0].amount).toBeCloseTo(10 - charged, 2);

            assertCleanConsole(messages);
        } finally {
            if (!cancelled) await staffCancel(staffToken, bookingId);
        }
    });
});
