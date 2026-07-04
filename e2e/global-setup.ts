import { FullConfig } from '@playwright/test';

const API = 'http://localhost:8099';

async function globalSetup(_config: FullConfig) {
    // Post-deploy smoke runs (SMOKE_BASE_URL set) target production and must
    // never seed test data. Bail out early — smoke.spec.ts is self-contained
    // and reads from process.env.SMOKE_BASE_URL directly.
    if (process.env.SMOKE_BASE_URL) {
        console.log(`SMOKE_BASE_URL=${process.env.SMOKE_BASE_URL} — skipping global-setup`);
        return;
    }

    // Wait for server to be ready
    for (let i = 0; i < 30; i++) {
        try {
            const resp = await fetch(`${API}/`);
            if (resp.ok) break;
        } catch {
            // Server not ready yet
        }
        await new Promise((r) => setTimeout(r, 500));
    }

    // Verify server is actually up
    const check = await fetch(`${API}/`);
    if (!check.ok) {
        throw new Error('Server not reachable at ' + API);
    }

    // Public registration was removed in #108 (invite-only onboarding). Seed
    // the bootstrap accounts (with passwords + roles) via the test-only
    // /api/test/seed-account fixture, which is mounted under SPINBIKE_TEST_MODE=1
    // (set for the E2E server) and never reachable in production.
    async function seedAccount(email: string, password: string, name: string, role: string) {
        const resp = await fetch(`${API}/api/test/seed-account`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ email, password, name, role }),
        });
        // 409 = already seeded (idempotent re-run); anything else is fatal.
        if (!resp.ok && resp.status !== 409) {
            throw new Error(`Failed to seed ${role} ${email}: ${resp.status} ${await resp.text()}`);
        }
    }

    await seedAccount('customer@test.com', 'password123', 'Test Customer', 'customer');
    await seedAccount('admin@test.com', 'admin123', 'Test Admin', 'admin');
    await seedAccount('staff@test.com', 'staff123', 'Test Staff', 'staff');

    // Login as admin to create test data
    const loginResp = await fetch(`${API}/api/auth/login`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email: 'admin@test.com', password: 'admin123' }),
    });
    if (!loginResp.ok) {
        throw new Error(`Failed to login as admin: ${loginResp.status} ${await loginResp.text()}`);
    }
    const loginData = await loginResp.json();
    const adminToken = loginData.token;

    const authHeaders = {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${adminToken}`,
    };

    // Create instructor
    await fetch(`${API}/api/admin/instructors`, {
        method: 'POST',
        headers: authHeaders,
        body: JSON.stringify({ name: 'Judita' }),
    });

    // Create class templates for multiple days
    // weekday: 0=Mon, 1=Tue, 2=Wed, 3=Thu, 4=Fri, 5=Sat, 6=Sun
    const templates = [
        { weekday: 0, start_time: '17:00', duration_minutes: 60, instructor_id: 1, capacity: 10 },
        { weekday: 1, start_time: '18:00', duration_minutes: 45, instructor_id: 1, capacity: 10 },
        { weekday: 2, start_time: '17:30', duration_minutes: 60, instructor_id: 1, capacity: 10 },
        { weekday: 3, start_time: '18:00', duration_minutes: 45, instructor_id: 1, capacity: 10 },
        { weekday: 4, start_time: '16:00', duration_minutes: 60, instructor_id: 1, capacity: 10 },
    ];

    for (const tmpl of templates) {
        await fetch(`${API}/api/admin/templates`, {
            method: 'POST',
            headers: authHeaders,
            body: JSON.stringify(tmpl),
        });
    }

    // Create named test users so search-by-name / company / phone / card_code can find them.
    const testUsers = [
        {
            name: 'Jana Testova',
            card_code: '70701001',
            initial_credit: 50.0,
            company: 'TestCorp',
            phone: '+421900111222',
        },
        {
            name: 'Petr Vzorny',
            card_code: '70702002',
            initial_credit: 25.0,
            company: 'TestCorp',
            phone: '+421900333444',
        },
        {
            name: 'Eva Novotna',
            card_code: '70703003',
            initial_credit: 10.0,
            company: 'OtherCo',
            phone: '+421900555666',
        },
    ];
    for (const user of testUsers) {
        await fetch(`${API}/api/users`, {
            method: 'POST',
            headers: authHeaders,
            body: JSON.stringify(user),
        });
    }

    // Post-V13 (#55): cards table is gone. Each card_code is now an attribute
    // on a `users` row. The customer (customer@test.com) does not own a card
    // code by default; tests that need a code-keyed customer create their own.

    // Create a service for payment tests
    await fetch(`${API}/api/admin/services`, {
        method: 'POST',
        headers: authHeaders,
        body: JSON.stringify({ name: 'Spinning', default_price: 120 }),
    });

    console.log('Global setup complete: users, templates, card, and service created.');
}

export default globalSetup;
