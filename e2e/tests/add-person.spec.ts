import { test, expect } from '@playwright/test';
import {
    setupConsoleCheck,
    assertCleanConsole,
    setEnglishLanguage,
    loginViaAPI,
} from './helpers';

const RUN_TAG = `ADDP${Math.random().toString(36).slice(2, 6).toUpperCase()}`;

test.describe('Add Person flow (#55)', () => {
    test('CEO can add a new person at the desk and find them in search', async ({ page, baseURL }) => {
        const messages = setupConsoleCheck(page);
        await loginViaAPI(page, baseURL!, 'staff@test.com', 'staff123');
        await setEnglishLanguage(page);

        await page.goto('/staff');
        await page.getByRole('button', { name: /add person/i }).click();

        const fullName = `Anna Test ${RUN_TAG}`;
        const email = `anna.${RUN_TAG.toLowerCase()}@e2e.local`;

        await page.getByLabel(/name/i).fill(fullName);
        await page.getByLabel(/email/i).fill(email);
        await page.getByLabel(/phone/i).fill('+421900111222');
        await page.getByLabel(/company/i).fill('TestCo');
        await page.getByTestId('add-person-submit').click();

        // Success banner
        await expect(page.locator('.alert-success'))
            .toContainText(`Person added`, { timeout: 5000 });

        // New person appears in search
        await page.locator('input[type="search"]').fill(RUN_TAG);
        await expect(page.locator('[data-testid="search-result"]', { hasText: fullName }))
            .toBeVisible({ timeout: 5000 });

        assertCleanConsole(messages);
    });

    test('Add Person rejects empty name', async ({ page, baseURL }) => {
        const messages = setupConsoleCheck(page);
        await loginViaAPI(page, baseURL!, 'staff@test.com', 'staff123');
        await setEnglishLanguage(page);

        await page.goto('/staff');
        await page.getByRole('button', { name: /add person/i }).click();
        await page.getByTestId('add-person-submit').click();

        await expect(page.locator('.alert-error'))
            .toContainText(/required/i, { timeout: 2000 });

        assertCleanConsole(messages);
    });
});
