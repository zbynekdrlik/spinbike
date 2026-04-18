import { defineConfig } from '@playwright/test';

export default defineConfig({
    testDir: './tests',
    timeout: 30000,
    retries: 0,
    globalSetup: './global-setup.ts',
    use: {
        baseURL: 'http://localhost:8099',
        headless: true,
    },
});
