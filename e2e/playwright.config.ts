import { defineConfig } from '@playwright/test';

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
});
