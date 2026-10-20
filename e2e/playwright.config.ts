import { defineConfig, devices } from '@playwright/test';

// #386: specs that must run under the REAL WebKit engine on an iPhone
// descriptor (touch, mobile viewport, iOS UA). Everything else stays on the
// default Chromium project, exactly as before.
const WEBKIT_IPHONE_SPECS = /ios-sheet-close\.spec\.ts$/;
// #385: specs that run in BOTH projects — Chromium AND the real-WebKit
// iPhone (the CEO runs the staff spinning screen on his phone).
const BOTH_ENGINE_SPECS = /staff-spinning\.spec\.ts$/;

export default defineConfig({
    testDir: './tests',
    timeout: 30000,
    retries: 0,
    // SQLite is single-writer. With the playwright default (≈ CPU/2 = 2
    // workers on GitHub Actions ubuntu-latest), concurrent E2E tests
    // racing for the SQLite write lock can occasionally exceed busy_timeout
    // and surface as a single SQLITE_BUSY (#45). Force serial execution
    // on CI to match the DB's single-writer model. Locally, devs keep the
    // playwright default for faster feedback.
    workers: process.env.CI ? 1 : undefined,
    globalSetup: './global-setup.ts',
    use: {
        baseURL: 'http://localhost:8099',
        headless: true,
    },
    projects: [
        {
            name: 'chromium',
            use: { browserName: 'chromium' },
            testIgnore: WEBKIT_IPHONE_SPECS,
        },
        {
            // CI installs the webkit browser for this project
            // (.github/workflows/ci.yml, test-ui job). The smoke jobs grep
            // for '@smoke' only, so they never launch webkit.
            name: 'webkit-iphone',
            use: { ...devices['iPhone 13'] },
            testMatch: [WEBKIT_IPHONE_SPECS, BOTH_ENGINE_SPECS],
        },
    ],
});
