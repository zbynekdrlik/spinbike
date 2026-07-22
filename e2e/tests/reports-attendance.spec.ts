import { test, expect, Page } from '@playwright/test';
import { setupConsoleCheck, assertCleanConsole, loginViaAPI } from './helpers';

const BASE_URL = 'http://localhost:8099';

interface ServiceLookup {
    spinning: number;
    fitness: number;
    monthly_pass: number;
    refreshments: number;
    card_activation_fee: number;
}

async function fetchServiceIds(token: string): Promise<ServiceLookup> {
    // Admin-only endpoint; the test logs in as admin@test.com so this is fine.
    // No public /api/services route exists — the staff dashboard uses
    // /api/admin/services too.
    const resp = await fetch(`${BASE_URL}/api/admin/services`, {
        headers: { Authorization: `Bearer ${token}` },
    });
    if (!resp.ok) throw new Error(`/api/admin/services failed: ${resp.status}`);
    const services: { id: number; name_en: string }[] = await resp.json();
    const find = (n: string) => {
        const s = services.find((x) => x.name_en === n);
        if (!s) throw new Error(`service "${n}" not in /api/admin/services`);
        return s.id;
    };
    return {
        spinning: find('Spinning'),
        fitness: find('Fitness'),
        monthly_pass: find('Monthly pass'),
        refreshments: find('Refreshments'),
        card_activation_fee: find('Card activation fee'),
    };
}

async function createUser(token: string, suffix: string, credit: number): Promise<number> {
    const resp = await fetch(`${BASE_URL}/api/users`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({
            name: `RA Reports${suffix}`,
            initial_credit: credit,
            card_code: `RA-${suffix}`,
        }),
    });
    if (!resp.ok) throw new Error(`createUser failed: ${resp.status} ${await resp.text()}`);
    const user = await resp.json();
    return user.id;
}

async function postCharge(token: string, userId: number, serviceId: number, amount: number) {
    const resp = await fetch(`${BASE_URL}/api/payments/charge`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ user_id: userId, amount, service_id: serviceId }),
    });
    if (!resp.ok) throw new Error(`charge failed: ${resp.status} ${await resp.text()}`);
}

async function postSellPass(token: string, userId: number, serviceId: number, price: number, validUntil: string) {
    const resp = await fetch(`${BASE_URL}/api/payments/sell-pass`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ user_id: userId, service_id: serviceId, price, valid_until: validUntil }),
    });
    if (!resp.ok) throw new Error(`sell-pass failed: ${resp.status} ${await resp.text()}`);
}

async function postLogVisit(token: string, userId: number, serviceId: number, force = false) {
    const resp = await fetch(`${BASE_URL}/api/payments/log-visit`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ user_id: userId, service_id: serviceId, force }),
    });
    if (!resp.ok) throw new Error(`log-visit failed: ${resp.status} ${await resp.text()}`);
}

async function postTopup(token: string, userId: number, amount: number) {
    const resp = await fetch(`${BASE_URL}/api/users/topup`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ user_id: userId, amount }),
    });
    if (!resp.ok) throw new Error(`topup failed: ${resp.status} ${await resp.text()}`);
}

test.describe('Reports — NAVSTEVY/ATTENDANCE KPI counts class visits only (#23)', () => {
    test('paid Fitness + paid Spinning + free pass-visits = 4; snacks/fees/passes/topups excluded', async ({ page }) => {
        const consoleMessages = setupConsoleCheck(page);

        // Reports endpoints require admin role.
        const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

        const services = await fetchServiceIds(token);
        const suffix = `${Date.now()}${Math.random().toString(36).slice(2, 6)}`;

        // Card with enough credit to cover all the charges below (5+5+2.50+2.50+3+35 = 53)
        // plus a 35 € pass sale; 100 keeps the math comfortably positive.
        const cardId = await createUser(token, suffix, 100.0);

        // Capture the before-count so the test is robust against pre-existing
        // class-visit transactions in the shared E2E DB. We assert that our
        // 4 seeded class-visit rows produced an exact +4 delta.
        const today = new Date().toISOString().slice(0, 10);
        const beforeResp = await fetch(`${BASE_URL}/api/reports/day?date=${today}`, {
            headers: { Authorization: `Bearer ${token}` },
        });
        expect(beforeResp.ok).toBe(true);
        const beforeJson = await beforeResp.json();
        const beforeAttendance: number = beforeJson.kpi.attendance;

        // 4 rows that SHOULD count toward attendance.
        await postCharge(token, cardId, services.fitness, 5.0);   // paid Fitness
        await postCharge(token, cardId, services.spinning, 5.0);  // paid Spinning
        // Sell a pass first so the log-visit calls reflect the real staff workflow,
        // even though the API itself doesn't require an active pass.
        // #251: the server validates valid_until against the Bratislava-LOCAL
        // "today" (today_bratislava()), not raw UTC — so a naive UTC+24h
        // "tomorrow" can land on the SAME Bratislava-local day the server
        // sees as "today" during the 00:00-02:00 Bratislava window (CI runs
        // in UTC), making a perfectly-valid future date get rejected as
        // "must be in the future". +48h is comfortably more than the max
        // real-world UTC/Bratislava offset (+2h CEST / +1h CET) plus a full
        // day, so it always lands strictly after Bratislava's current day
        // regardless of when in the day this test happens to run.
        const tomorrow = new Date(Date.now() + 2 * 24 * 3600_000).toISOString().slice(0, 10);
        await postSellPass(token, cardId, services.monthly_pass, 35.0, tomorrow); // counts toward passes_sold, NOT attendance
        // #234: log-visit 409s when this user already has a same-day
        // class-visit event — and the paid Fitness/Spinning charges above
        // already count as one (canonical attendance definition). Both
        // calls here are a genuine, intentional second/third entry for the
        // same day (this test seeds several class-visit events on purpose
        // to prove the KPI sums all of them) — force:true is the documented
        // legitimate use, not a bypass of the guard's intent.
        await postLogVisit(token, cardId, services.fitness, true);   // free Fitness visit
        await postLogVisit(token, cardId, services.spinning, true);  // free Spinning visit

        // 4 more rows that should NOT count toward attendance (in addition to the pass-sale above).
        await postCharge(token, cardId, services.refreshments, 2.50);  // snack #1
        await postCharge(token, cardId, services.refreshments, 2.50);  // snack #2 — discriminator
        await postCharge(token, cardId, services.card_activation_fee, 3.0); // card fee
        await postTopup(token, cardId, 10.0);                           // topup, no service

        // After-count via the JSON API.
        const afterResp = await fetch(`${BASE_URL}/api/reports/day?date=${today}`, {
            headers: { Authorization: `Bearer ${token}` },
        });
        expect(afterResp.ok).toBe(true);
        const afterJson = await afterResp.json();
        expect(afterJson.kpi.attendance - beforeAttendance).toBe(4);

        // Drive the UI: navigate to /reports and read the kpi-attendance tile.
        // Use the date filter so the UI reflects the same day as the JSON probe.
        await page.goto(`/reports?date=${today}`);
        const kpiAttendance = page.locator('[data-testid="kpi-attendance"]');
        await expect(kpiAttendance).toBeVisible();
        // The displayed value equals the after-count (which is beforeAttendance + 4).
        await expect(kpiAttendance).toContainText(String(afterJson.kpi.attendance));

        assertCleanConsole(consoleMessages);
    });
});
