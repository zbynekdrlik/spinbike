import { FullConfig } from '@playwright/test';
import { execSync } from 'child_process';

const API = 'http://localhost:8099';
const DB_PATH = process.env.DATABASE_PATH || '/tmp/spinbike-e2e-test.db';

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

    // Register customer
    const custResp = await fetch(`${API}/api/auth/register`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
            email: 'customer@test.com',
            password: 'password123',
            name: 'Test Customer',
        }),
    });
    if (!custResp.ok && custResp.status !== 409) {
        throw new Error(`Failed to register customer: ${custResp.status} ${await custResp.text()}`);
    }

    // Register admin (starts as customer)
    const adminResp = await fetch(`${API}/api/auth/register`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
            email: 'admin@test.com',
            password: 'admin123',
            name: 'Test Admin',
        }),
    });
    if (!adminResp.ok && adminResp.status !== 409) {
        throw new Error(`Failed to register admin: ${adminResp.status} ${await adminResp.text()}`);
    }

    // Register staff (starts as customer)
    const staffResp = await fetch(`${API}/api/auth/register`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
            email: 'staff@test.com',
            password: 'staff123',
            name: 'Test Staff',
        }),
    });
    if (!staffResp.ok && staffResp.status !== 409) {
        throw new Error(`Failed to register staff: ${staffResp.status} ${await staffResp.text()}`);
    }

    // Promote admin and staff roles via sqlite3
    execSync(`sqlite3 "${DB_PATH}" "UPDATE users SET role='admin' WHERE email='admin@test.com'"`);
    execSync(`sqlite3 "${DB_PATH}" "UPDATE users SET role='staff' WHERE email='staff@test.com'"`);

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

    // Create named test cards so search-by-name / company / phone can find them.
    const testCards = [
        {
            barcode: '70701001',
            initial_credit: 50.0,
            first_name: 'Jana',
            last_name: 'Testova',
            company: 'TestCorp',
            phone: '+421900111222',
        },
        {
            barcode: '70702002',
            initial_credit: 25.0,
            first_name: 'Petr',
            last_name: 'Vzorny',
            company: 'TestCorp',
            phone: '+421900333444',
        },
        {
            barcode: '70703003',
            initial_credit: 10.0,
            first_name: 'Eva',
            last_name: 'Novotna',
            company: 'OtherCo',
            phone: '+421900555666',
        },
    ];
    for (const card of testCards) {
        await fetch(`${API}/api/cards/activate`, {
            method: 'POST',
            headers: authHeaders,
            body: JSON.stringify(card),
        });
    }

    // Link Jana's card to the customer user so card-centric booking flows
    // (which write bookings referencing the card's user_id) work in E2E.
    execSync(`sqlite3 "${DB_PATH}" "UPDATE cards SET user_id = (SELECT id FROM users WHERE email='customer@test.com') WHERE barcode='70701001'"`);

    // Create a service for payment tests
    await fetch(`${API}/api/admin/services`, {
        method: 'POST',
        headers: authHeaders,
        body: JSON.stringify({ name: 'Spinning', default_price: 120 }),
    });

    console.log('Global setup complete: users, templates, card, and service created.');
}

export default globalSetup;
