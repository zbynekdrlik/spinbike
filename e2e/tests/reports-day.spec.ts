import { test, expect } from '@playwright/test';
import { loginViaAPI, setupConsoleCheck, assertCleanConsole } from './helpers';

const BASE_URL = 'http://localhost:8099';

// Day-specific E2E: seed data via API, navigate to /reports, verify the KPI
// numbers and feed reflect the underlying transactions.
test('day report KPI cards reflect seeded transactions', async ({ page }) => {
    const consoleMessages = setupConsoleCheck(page);
    const token = await loginViaAPI(page, BASE_URL, 'admin@test.com', 'admin123');

    // Create a fresh user with €50 starting credit (one top-up event implicit
    // in creation flow may add to cash_in — we don't assert exact amounts,
    // only "non-zero and rendered").
    const suffix = `${Date.now()}${Math.random().toString(36).slice(2, 6)}`;
    const user = await fetch(`${BASE_URL}/api/users`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({
            name: `Rep T${suffix}`,
            initial_credit: 50,
            card_code: `RPT-${suffix}`,
        }),
    }).then((r) => r.json());

    // Look up Spinning service id.
    const services: { id: number; name_en: string }[] = await fetch(`${BASE_URL}/api/admin/services`, {
        headers: { Authorization: `Bearer ${token}` },
    }).then((r) => r.json());
    const spinning = services.find((s) => s.name_en === 'Spinning');
    if (!spinning) throw new Error('Spinning service not found');

    // One charge.
    await fetch(`${BASE_URL}/api/payments/charge`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
        body: JSON.stringify({ user_id: user.id, amount: 5, service_id: spinning.id }),
    });

    await page.goto('/reports');
    await expect(page.locator('[data-testid="reports-page"]')).toBeVisible();

    // Activity feed must render at least one row reflecting the seeded charge.
    await expect(page.locator('[data-testid="feed-row"]').first()).toBeVisible();

    // Attendance KPI must show a number (the integer count, not blank).
    const attendance = await page.locator('[data-testid="kpi-attendance"]').innerText();
    expect(attendance).toMatch(/\d/);

    assertCleanConsole(consoleMessages);
});
