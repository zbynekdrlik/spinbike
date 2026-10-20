/**
 * Shared spinning-booking fixtures (#382).
 *
 * Since #382 R4 the real `POST /api/bookings` refuses a class that already
 * started, and the 09:00 binding cutoff decides whether a customer may still
 * cancel. A spec that used to grab "the first class of the current week" can
 * therefore hit a past class (Monday's, when the suite runs on Wednesday) or,
 * on Friday-Sunday, find NO bookable class in the current week at all. These
 * helpers pick by the server's own `started` / `cancel_open` flags (computed
 * in Europe/Bratislava time — never re-derived here) and seed past-date
 * bookings through the test-only `/api/test/seed-booking` fixture.
 */
import { Page, expect } from '@playwright/test';
import { bratislavaToday, bratislavaDateOffset } from './helpers';

export const BASE_URL = 'http://localhost:8099';

export interface ClassOccurrence {
    template_id: number;
    date: string;
    start_time: string;
    weekday: number;
    cancelled: boolean;
    booked: number;
    capacity: number;
    started: boolean;
    cancel_open: boolean;
    user_booked: boolean;
}

/** Monday..Sunday (YYYY-MM-DD) of the gym-local week `weekOffset` weeks ahead. */
export function weekRange(weekOffset = 0): { from: string; to: string; dates: string[] } {
    const [y, m, d] = bratislavaToday().split('-').map(Number);
    const today = new Date(Date.UTC(y, m - 1, d));
    const sinceMonday = (today.getUTCDay() + 6) % 7;
    const dates: string[] = [];
    for (let i = 0; i < 7; i++) {
        const day = new Date(Date.UTC(y, m - 1, d - sinceMonday + 7 * weekOffset + i));
        dates.push(day.toISOString().slice(0, 10));
    }
    return { from: dates[0], to: dates[6], dates };
}

export async function apiLogin(email: string, password: string): Promise<string> {
    const resp = await fetch(`${BASE_URL}/api/auth/login`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email, password }),
    });
    if (!resp.ok) throw new Error(`login ${email} failed: ${resp.status} ${await resp.text()}`);
    return (await resp.json()).token as string;
}

export async function listClasses(from: string, to: string, token?: string): Promise<ClassOccurrence[]> {
    const resp = await fetch(`${BASE_URL}/api/classes?from=${from}&to=${to}`, {
        headers: token ? { Authorization: `Bearer ${token}` } : {},
    });
    if (!resp.ok) throw new Error(`list classes failed: ${resp.status} ${await resp.text()}`);
    return resp.json();
}

/**
 * A class in the CURRENT gym-local week that has not started yet (bookable
 * through the real route). On Friday-Sunday every regular class of the week
 * is already over, so a temporary late-evening (23:50) template is created on
 * today's weekday; `cleanup` deactivates it again. Known limit: a run between
 * 23:50 and midnight Bratislava finds that class started too.
 */
export async function bookableSlotThisWeek(
    adminToken: string,
): Promise<{ slot: ClassOccurrence; cleanup: () => Promise<void> }> {
    const { from, to } = weekRange(0);
    const existing = (await listClasses(from, to)).find(
        (c) => !c.cancelled && !c.started && c.booked < c.capacity,
    );
    if (existing) return { slot: existing, cleanup: async () => {} };

    const today = bratislavaToday();
    const [y, m, d] = today.split('-').map(Number);
    const weekday = (new Date(Date.UTC(y, m - 1, d)).getUTCDay() + 6) % 7;
    const create = await fetch(`${BASE_URL}/api/admin/templates`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
        body: JSON.stringify({ weekday, start_time: '23:50', duration_minutes: 5, capacity: 19, instructor_id: null }),
    });
    if (!create.ok) throw new Error(`create template failed: ${create.status} ${await create.text()}`);
    const { id } = await create.json();
    const slot = (await listClasses(today, today)).find((c) => c.template_id === id);
    if (!slot) throw new Error(`temporary template ${id} not listed on ${today}`);
    return {
        slot,
        cleanup: async () => {
            const off = await fetch(`${BASE_URL}/api/admin/templates/${id}`, {
                method: 'PUT',
                headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${adminToken}` },
                body: JSON.stringify({ active: false }),
            });
            if (!off.ok) throw new Error(`deactivating temporary template ${id} failed: ${off.status}`);
        },
    };
}

/**
 * A future occurrence (this or next week) a CUSTOMER can still cancel
 * (before its 09:00 cutoff), not full — plus which week (0/1) the schedule
 * page must show to reach it.
 */
export async function cancellableSlotWithinTwoWeeks(): Promise<{ slot: ClassOccurrence; week: 0 | 1 }> {
    for (const week of [0, 1] as const) {
        const { from, to } = weekRange(week);
        const slot = (await listClasses(from, to)).find(
            (c) => !c.cancelled && !c.started && c.cancel_open && c.booked < c.capacity,
        );
        if (slot) return { slot, week };
    }
    throw new Error(`no cancellable class between ${bratislavaToday()} and ${bratislavaDateOffset(14)}`);
}

/** Insert a booking on ANY date via the test-only fixture (staff token). */
export async function seedBooking(
    staffToken: string,
    templateId: number,
    date: string,
    userId: number,
    source: 'manual' | 'persistent' = 'manual',
): Promise<number> {
    const resp = await fetch(`${BASE_URL}/api/test/seed-booking`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${staffToken}` },
        body: JSON.stringify({ template_id: templateId, date, user_id: userId, source }),
    });
    if (resp.status !== 201) throw new Error(`seed-booking failed: ${resp.status} ${await resp.text()}`);
    return (await resp.json()).id as number;
}

/**
 * Write the binding charge for one booking now (test-only fixture). The E2E
 * server's own 60-second charger may have charged a past-dated booking first
 * (`charged: false` then) — either way the booking ends up charged.
 */
export async function chargeBooking(staffToken: string, bookingId: number): Promise<void> {
    const resp = await fetch(`${BASE_URL}/api/test/charge-booking`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${staffToken}` },
        body: JSON.stringify({ booking_id: bookingId }),
    });
    if (!resp.ok) throw new Error(`charge-booking failed: ${resp.status} ${await resp.text()}`);
}

/** Staff cancel (any time; refunds a charged booking). Best-effort for cleanup. */
export async function staffCancel(staffToken: string, bookingId: number): Promise<number> {
    const resp = await fetch(`${BASE_URL}/api/bookings/${bookingId}`, {
        method: 'DELETE',
        headers: { Authorization: `Bearer ${staffToken}` },
    });
    return resp.status;
}

/** One row of the staff participants endpoint (#385 shape). */
export interface ParticipantRow {
    booking_id: number;
    user_id: number;
    user_name: string;
    source: 'manual' | 'persistent';
    refundable: boolean;
    phone: string | null;
    card_code: string | null;
    credit: number;
    charged: boolean;
    charged_amount: number | null;
}

export async function listParticipants(
    staffToken: string,
    templateId: number,
    date: string,
): Promise<ParticipantRow[]> {
    const resp = await fetch(`${BASE_URL}/api/classes/${templateId}/${date}/participants`, {
        headers: { Authorization: `Bearer ${staffToken}` },
    });
    if (!resp.ok) throw new Error(`participants failed: ${resp.status} ${await resp.text()}`);
    return resp.json();
}

/**
 * Open the staff spinning screen (#385) and tap the grid cell of one class
 * occurrence; resolves once that occurrence's detail table has loaded. The
 * occurrence must lie inside the 8-week grid (current week + 7).
 */
export async function openStaffSpinningCell(page: Page, templateId: number, date: string): Promise<void> {
    if (!page.url().includes('/staff/spinning')) {
        await page.goto('/staff/spinning');
    }
    const cell = page.locator(`[data-testid="occ-cell-${templateId}-${date}"]`);
    await expect(cell).toBeVisible({ timeout: 10000 });
    await cell.click();
    await expect(cell).toHaveAttribute('aria-pressed', 'true');
    const detail = page.locator('[data-testid="spin-detail"]');
    await expect(detail).toHaveAttribute('data-date', date);
    await expect(detail).toHaveAttribute('data-template-id', String(templateId));
    // The table finished loading: rows, or the "nobody is booked" row (never
    // the transient loading text).
    await expect(
        detail.locator('[data-testid^="spin-row-"], [data-testid="spin-participants-empty"]').first(),
    ).toBeVisible({ timeout: 10000 });
    await expect(detail.locator('[data-testid="spin-participants-empty"]')).not.toHaveText(/Loading/, {
        timeout: 10000,
    });
}

/** Day-picker button index (0 = Monday) of a YYYY-MM-DD date. */
export function mondayFirstIndex(date: string): number {
    const [y, m, d] = date.split('-').map(Number);
    return (new Date(Date.UTC(y, m - 1, d)).getUTCDay() + 6) % 7;
}
