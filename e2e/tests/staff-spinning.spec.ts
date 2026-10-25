import { test, expect, Page } from '@playwright/test';
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
    listClasses,
    listParticipants,
    openStaffSpinningCell,
    seedBooking,
    staffCancel,
    weekRange,
} from './spinning-fixtures';

/**
 * #385 — the staff spinning screen (`/staff/spinning`, spec
 * docs/superpowers/specs/2026-10-20-staff-spinning-screen-design.md):
 * an 8-week occupancy grid + the class detail TABLE, from which staff enrol a
 * person once or permanently, unbook them and end a permanent slot.
 *
 * Runs in BOTH Playwright projects (chromium + the real-WebKit iPhone,
 * playwright.config.ts) — the CEO uses it on his phone.
 */

/**
 * A class NEXT week (never started, cutoff ahead) with room for two more,
 * whose same weekly slot the week after is also open — the "permanent"
 * enrolment must show up in both.
 */
async function slotWithNextWeek(): Promise<{ slot: ClassOccurrence; next: ClassOccurrence }> {
    const w1 = weekRange(1);
    const w2 = weekRange(2);
    const week1 = await listClasses(w1.from, w1.to);
    const week2 = await listClasses(w2.from, w2.to);
    for (const slot of week1) {
        if (slot.cancelled || slot.started || slot.booked + 2 > slot.capacity) continue;
        const next = week2.find(
            (c) => c.template_id === slot.template_id && !c.cancelled && c.booked + 1 <= c.capacity,
        );
        if (next) return { slot, next };
    }
    throw new Error(`no open class pair in ${w1.from}..${w2.to}`);
}

async function personalSlots(staffToken: string, userId: number): Promise<number[]> {
    const resp = await fetch(`${BASE_URL}/api/users/${userId}/persistent-bookings`, {
        headers: { Authorization: `Bearer ${staffToken}` },
    });
    if (!resp.ok) throw new Error(`persistent list failed: ${resp.status}`);
    const rows: Array<{ template_id: number }> = await resp.json();
    return rows.map((r) => r.template_id);
}

async function endPersistent(staffToken: string, userId: number, templateId: number): Promise<void> {
    await fetch(`${BASE_URL}/api/users/${userId}/persistent-bookings/${templateId}`, {
        method: 'DELETE',
        headers: { Authorization: `Bearer ${staffToken}` },
    }).catch(() => {});
}

/** Search a customer in "Pridat" and return their result row. */
async function findInAddPerson(page: Page, cardCode: string, userId: number) {
    const add = page.locator('[data-testid="spin-add"]');
    await add.locator('[data-testid="spin-add-search"]').fill(cardCode);
    const hit = add.locator(`[data-testid="spin-add-hit-${userId}"]`);
    await expect(hit).toBeVisible({ timeout: 10000 });
    return hit;
}

function rowOf(page: Page, name: string) {
    return page.locator('[data-testid="spin-detail"] [data-testid^="spin-row-"]', { hasText: name });
}

test.describe('Staff spinning screen (#385)', () => {
    test('the grid shows 8 weeks and a cell opens a table matching the API', async ({ page }) => {
        const messages = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { slot } = await slotWithNextWeek();
        const customer = await createUniqueUser(token, 3, 'SG');
        const seed = await fetch(`${BASE_URL}/api/bookings`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
            body: JSON.stringify({ template_id: slot.template_id, date: slot.date, user_id: customer.user_id }),
        });
        expect(seed.status).toBe(201);
        const bookingId = (await seed.json()).id as number;
        try {
            await page.goto('/staff/spinning');
            await expect(page.locator('[data-testid="spin-page-title"]')).toHaveText('Spinning');
            await expect(page.locator('[data-testid^="occ-week-"]')).toHaveCount(8, { timeout: 10000 });
            await expect(page.locator('[data-testid="nav-schedule"]')).toHaveAttribute('aria-current', 'page');

            await openStaffSpinningCell(page, slot.template_id, slot.date);
            const api = await listParticipants(token, slot.template_id, slot.date);
            const rows = page.locator('[data-testid="spin-detail"] [data-testid^="spin-row-"]');
            await expect(rows).toHaveCount(api.length);
            await expect(page.locator('[data-testid="spin-detail-count"]')).toContainText(
                `${api.length}/${slot.capacity}`,
            );
            for (const p of api) {
                await expect(page.locator(`[data-testid="spin-name-${p.booking_id}"]`)).toHaveText(p.user_name);
            }

            // Our customer's row: one-off, waiting for the 09:00 charge,
            // current credit, name links to the desk detail by card code.
            await expect(page.locator(`[data-testid="spin-kind-${bookingId}"]`)).toHaveText('one-off');
            await expect(page.locator(`[data-testid="spin-state-${bookingId}"]`)).toHaveText('waiting for 9:00');
            await expect(page.locator(`[data-testid="spin-credit-${bookingId}"]`)).toHaveText('3.00 €');
            await expect(page.locator(`[data-testid="spin-name-${bookingId}"]`)).toHaveAttribute(
                'href',
                `/staff?card=${customer.card_code}`,
            );

            // Tapping another cell keeps the SAME table node (no rebuild ->
            // a phone keeps its horizontal scroll position).
            const grid = page.locator('[data-testid="occ-grid"]');
            await grid.evaluate((el) => {
                (el as HTMLElement).dataset.probe = 'kept';
            });
            const other = page
                .locator(
                    `[data-testid^="occ-cell-"]:not([data-testid="occ-cell-${slot.template_id}-${slot.date}"])`,
                )
                .first();
            await other.click();
            await expect(other).toHaveAttribute('aria-pressed', 'true');
            await expect(grid).toHaveAttribute('data-probe', 'kept');
            // The selection is kept in the address bar for a reload.
            const otherId = (await other.getAttribute('data-testid'))!.replace('occ-cell-', '');
            const [otherTid, ...otherDate] = otherId.split('-');
            await expect(page).toHaveURL(
                new RegExp(`/staff/spinning\\?t=${otherTid}&d=${otherDate.join('-')}$`),
            );

            // A deep link to a class outside the grid shows the hint, not a blank page.
            await page.goto(`/staff/spinning?t=999999&d=${slot.date}`);
            await expect(page.locator('[data-testid^="occ-week-"]')).toHaveCount(8, { timeout: 10000 });
            await expect(page.locator('[data-testid="spin-pick-hint"]')).toBeVisible();
            await expect(page.locator('[data-testid="spin-detail"]')).toHaveCount(0);

            // A deep link opens the class directly.
            await page.goto(`/staff/spinning?t=${slot.template_id}&d=${slot.date}`);
            await expect(page.locator('[data-testid="spin-detail"]')).toHaveAttribute('data-date', slot.date);
            await expect(page.locator(`[data-testid="spin-name-${bookingId}"]`)).toHaveText(customer.name);
            await expect(
                page.locator(`[data-testid="occ-cell-${slot.template_id}-${slot.date}"]`),
            ).toHaveAttribute('aria-pressed', 'true');

            // The name opens the customer's detail at the desk.
            await page.locator(`[data-testid="spin-name-${bookingId}"]`).click();
            await expect(page).toHaveURL(new RegExp(`/staff\\?card=${customer.card_code}$`));
            await expect(page.locator('[data-testid="action-panel"]')).toBeVisible({ timeout: 10000 });
            assertCleanConsole(messages);
        } finally {
            await staffCancel(token, bookingId);
        }
    });

    test('a /schedule summary row opens that class on the spinning screen', async ({ page }) => {
        const messages = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { from, to } = weekRange(0);
        const slot = (await listClasses(from, to)).find((c) => !c.cancelled);
        if (!slot) throw new Error(`no class in ${from}..${to}`);

        await page.goto('/schedule');
        // In-app (client-side) navigation — the router renders the new route
        // before it pushes the URL, which the preselect must survive.
        await page.locator(`[data-testid="roster-class-${slot.template_id}-${slot.date}"]`).click();
        await expect(page).toHaveURL(/\/staff\/spinning\?/);
        await expect(page.locator('[data-testid="spin-detail"]')).toHaveAttribute('data-date', slot.date, {
            timeout: 10000,
        });
        await expect(
            page.locator(`[data-testid="occ-cell-${slot.template_id}-${slot.date}"]`),
        ).toHaveAttribute('aria-pressed', 'true');
        assertCleanConsole(messages);
    });

    test('enrol once and permanently, unbook, end the permanent slot', async ({ page }) => {
        const messages = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { slot, next } = await slotWithNextWeek();
        const once = await createUniqueUser(token, 0, 'SO');
        const regular = await createUniqueUser(token, 0, 'SR');
        try {
            await openStaffSpinningCell(page, slot.template_id, slot.date);
            const cell = page.locator(`[data-testid="occ-cell-${slot.template_id}-${slot.date}"]`);
            const before = (await listParticipants(token, slot.template_id, slot.date)).length;

            // "Len na tento termin" -> the row appears, the cell counts +1.
            const onceHit = await findInAddPerson(page, once.card_code, once.user_id);
            await onceHit.locator(`[data-testid="spin-add-once-${once.user_id}"]`).click();
            await expect(rowOf(page, once.name)).toHaveCount(1, { timeout: 10000 });
            await expect(cell).toHaveText(new RegExp(`^${before + 1}/${slot.capacity}`));
            const afterOnce = await listParticipants(token, slot.template_id, slot.date);
            const onceRow = afterOnce.find((p) => p.user_id === once.user_id);
            expect(onceRow?.source, 'one-off booking').toBe('manual');

            // "Natrvalo" -> the row here AND in the same slot next week.
            const regHit = await findInAddPerson(page, regular.card_code, regular.user_id);
            await regHit.locator(`[data-testid="spin-add-persistent-${regular.user_id}"]`).click();
            await expect(rowOf(page, regular.name)).toHaveCount(1, { timeout: 10000 });
            await expect(rowOf(page, regular.name)).toContainText('permanent');
            expect(await personalSlots(token, regular.user_id)).toContain(slot.template_id);
            await openStaffSpinningCell(page, next.template_id, next.date);
            await expect(rowOf(page, regular.name)).toHaveCount(1, { timeout: 10000 });
            const nextRows = await listParticipants(token, next.template_id, next.date);
            expect(nextRows.find((p) => p.user_id === regular.user_id)?.source).toBe('persistent');

            // A customer already in the class is offered no booking (#378).
            const already = await findInAddPerson(page, regular.card_code, regular.user_id);
            await expect(already.locator(`[data-testid="spin-add-already-${regular.user_id}"]`)).toBeVisible();
            await expect(already.locator('button')).toHaveCount(0);

            // "Odhlasit" the one-off customer (confirmed inline) -> row gone.
            await openStaffSpinningCell(page, slot.template_id, slot.date);
            await page.locator(`[data-testid="spin-unbook-${onceRow!.booking_id}"]`).click();
            await expect(page.locator('[data-testid="spin-confirm-text"]')).toContainText(once.name);
            await page.locator('[data-testid="spin-confirm-yes"]').click();
            await expect(rowOf(page, once.name)).toHaveCount(0, { timeout: 10000 });
            await expect(cell).toHaveText(new RegExp(`^${before + 1}/${slot.capacity}`));
            const afterUnbook = await listParticipants(token, slot.template_id, slot.date);
            expect(afterUnbook.some((p) => p.user_id === once.user_id), 'one-off booking cancelled').toBe(false);

            // "Ukoncit natrvalo" -> the regular disappears from both weeks.
            const regRow = afterUnbook.find((p) => p.user_id === regular.user_id)!;
            await page.locator(`[data-testid="spin-end-persistent-${regRow.booking_id}"]`).click();
            await expect(page.locator('[data-testid="spin-confirm-text"]')).toContainText(
                'Future classes are cancelled too',
            );
            await page.locator('[data-testid="spin-confirm-yes"]').click();
            await expect(rowOf(page, regular.name)).toHaveCount(0, { timeout: 10000 });
            await expect(cell).toHaveText(new RegExp(`^${before}/${slot.capacity}`));
            await openStaffSpinningCell(page, next.template_id, next.date);
            await expect(rowOf(page, regular.name)).toHaveCount(0);
            expect(await personalSlots(token, regular.user_id)).not.toContain(slot.template_id);
            const nextAfter = await listParticipants(token, next.template_id, next.date);
            expect(nextAfter.some((p) => p.user_id === regular.user_id), 'future booking cancelled').toBe(false);

            assertCleanConsole(messages);
        } finally {
            await endPersistent(token, regular.user_id, slot.template_id);
            for (const date of [slot.date, next.date]) {
                const rows = await listParticipants(token, slot.template_id, date).catch(() => []);
                for (const p of rows) {
                    if (p.user_id === once.user_id || p.user_id === regular.user_id) {
                        await staffCancel(token, p.booking_id);
                    }
                }
            }
        }
    });

    test('grid tints keep text readable (>= 4.5:1) in dark and light', async ({ page }) => {
        const messages = setupConsoleCheck(page);
        await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        for (const scheme of ['dark', 'light'] as const) {
            await page.emulateMedia({ colorScheme: scheme });
            await page.goto('/staff/spinning');
            await expect(page.locator('[data-testid^="occ-week-"]')).toHaveCount(8, { timeout: 10000 });
            // Every fill level / state, rendered with the page's real CSS
            // inside a real grid cell — independent of the seeded bookings.
            const variants = [
                'occ-cell occ-cell--low',
                'occ-cell occ-cell--mid',
                'occ-cell occ-cell--full',
                'occ-cell occ-cell--low occ-cell--past',
                'occ-cell occ-cell--mid occ-cell--past',
                'occ-cell occ-cell--full occ-cell--past',
                'occ-cell occ-cell--low occ-cell--cancelled',
            ];
            await page.evaluate((classes: string[]) => {
                const td = document.querySelector('[data-testid="occ-grid"] tbody td');
                if (!td) throw new Error('no grid cell');
                classes.forEach((c, i) => {
                    const b = document.createElement('button');
                    b.className = c;
                    b.textContent = '12/19';
                    b.setAttribute('data-probe', String(i));
                    td.appendChild(b);
                });
            }, variants);
            for (let i = 0; i < variants.length; i++) {
                const ratio = await contrastOf(page.locator(`[data-probe="${i}"]`));
                expect(ratio, `${scheme}: ${variants[i]}`).toBeGreaterThanOrEqual(4.5);
            }
            // The real cells too.
            const real = page.locator('[data-testid^="occ-cell-"]');
            const n = Math.min(await real.count(), 8);
            expect(n).toBeGreaterThan(0);
            for (let i = 0; i < n; i++) {
                expect(await contrastOf(real.nth(i)), `${scheme}: real cell ${i}`).toBeGreaterThanOrEqual(4.5);
            }
        }
        assertCleanConsole(messages);
    });

    // #389 (owner relays CEO Stevo: "Trvala rezervacia na nb nevidno maly
    // kontrast"): the detail's action buttons were `.btn--ghost` — a
    // transparent fill, a `--border` outline and a muted label — and on a
    // laptop did not read as buttons at all. Every action button must have a
    // readable label (>= 4.5:1) AND a visible boundary (fill or border
    // >= 3:1 against the card behind it), in both colour schemes.
    test('action buttons are visible: label >= 4.5:1, boundary >= 3:1 (dark + light)', async ({ page }) => {
        const messages = setupConsoleCheck(page);
        const token = await loginViaAPI(page, BASE_URL, 'staff@test.com', 'staff123');
        const { slot } = await slotWithNextWeek();
        const regular = await createUniqueUser(token, 0, 'SV');
        const searched = await createUniqueUser(token, 0, 'SW');
        const bookingId = await seedBooking(token, slot.template_id, slot.date, regular.user_id, 'persistent');
        try {
            for (const scheme of ['dark', 'light'] as const) {
                await page.emulateMedia({ colorScheme: scheme });
                await page.goto('/staff/spinning');
                await openStaffSpinningCell(page, slot.template_id, slot.date);
                await findInAddPerson(page, searched.card_code, searched.user_id);
                await page.locator('[data-testid="spin-cancel-class"]').click();
                await expect(page.locator('[data-testid="spin-confirm"]')).toBeVisible();
                const buttons = [
                    'spin-cancel-class',
                    'spin-confirm-no',
                    `spin-unbook-${bookingId}`,
                    `spin-end-persistent-${bookingId}`,
                    `spin-add-persistent-${searched.user_id}`,
                    `spin-add-once-${searched.user_id}`,
                ];
                for (const id of buttons) {
                    const btn = page.locator(`[data-testid="${id}"]`);
                    await expect(btn, `${scheme}: ${id} rendered`).toHaveCount(1);
                    expect(await contrastOf(btn), `${scheme}: ${id} label`).toBeGreaterThanOrEqual(4.5);
                    expect(await boundaryContrastOf(btn), `${scheme}: ${id} boundary`).toBeGreaterThanOrEqual(3);
                }
                await page.locator('[data-testid="spin-confirm-no"]').click();
                await expect(page.locator('[data-testid="spin-confirm"]')).toHaveCount(0);
            }
            assertCleanConsole(messages);
        } finally {
            await staffCancel(token, bookingId);
        }
    });
});

/**
 * WCAG contrast of a button's BOUNDARY against the surface behind it: the
 * better of its own (composited) fill and its border colour, each against the
 * effective background of its parent (WCAG 1.4.11 non-text contrast, 3:1).
 */
async function boundaryContrastOf(btn: import('@playwright/test').Locator): Promise<number> {
    return btn.evaluate((el) => {
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
        const effectiveBg = (start: Element | null): RGBA => {
            const layers: RGBA[] = [];
            let node = start;
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
            return bg;
        };
        const lum = (c: RGBA) => {
            const ch = c.slice(0, 3).map((v) => {
                const x = v / 255;
                return x <= 0.03928 ? x / 12.92 : Math.pow((x + 0.055) / 1.055, 2.4);
            });
            return 0.2126 * ch[0] + 0.7152 * ch[1] + 0.0722 * ch[2];
        };
        const ratio = (a: RGBA, b: RGBA) => {
            const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
            return (hi + 0.05) / (lo + 0.05);
        };
        const behind = effectiveBg(el.parentElement);
        const style = getComputedStyle(el);
        const fill = over(parse(style.backgroundColor), behind);
        const borderWidth = parseFloat(style.borderTopWidth) || 0;
        const border = borderWidth > 0 ? over(parse(style.borderTopColor), behind) : behind;
        return Math.max(ratio(fill, behind), ratio(border, behind));
    });
}
