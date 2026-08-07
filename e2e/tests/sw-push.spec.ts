import { test, expect } from '@playwright/test';
import { readFileSync } from 'fs';
import * as path from 'path';
import vm from 'vm';

/**
 * Unit coverage for `spinbike-ui/sw.js`'s `push` + `notificationclick`
 * listeners (#264), added alongside the existing fetch/cache handlers
 * (see `sw-cache.spec.ts`). Same technique: load the REAL shipped sw.js
 * into a mocked ServiceWorkerGlobalScope via `vm` and drive synthetic
 * events — deterministic and doesn't need a live browser Push/Notification
 * round-trip (which the E2E subscribe-button test intentionally avoids
 * too — see `push-notifications.spec.ts`'s own doc comment for why).
 */

const SW_PATH = path.join(__dirname, '..', '..', 'spinbike-ui', 'sw.js');

interface ShowNotificationCall {
    title: string;
    options: { body?: string; icon?: string; badge?: string };
}

interface MockWindowClient {
    url: string;
    focus(): Promise<MockWindowClient>;
}

function loadSW() {
    const source = readFileSync(SW_PATH, 'utf-8');

    const showNotificationCalls: ShowNotificationCall[] = [];
    const handlers: Record<string, (event: unknown) => void> = {};

    let matchAllResult: MockWindowClient[] = [];
    let openedWindowUrl: string | null = null;

    const self = {
        addEventListener(type: string, fn: (event: unknown) => void) {
            handlers[type] = fn;
        },
        registration: {
            showNotification(title: string, options: ShowNotificationCall['options']) {
                showNotificationCalls.push({ title, options });
                return Promise.resolve();
            },
        },
    };

    const clientsGlobal = {
        matchAll: (_opts: unknown) => Promise.resolve(matchAllResult),
        openWindow: (url: string) => {
            openedWindowUrl = url;
            return Promise.resolve(null);
        },
    };

    const sandbox: Record<string, unknown> = {
        self,
        clients: clientsGlobal,
        URL,
        Promise,
        console,
    };
    vm.createContext(sandbox);
    vm.runInContext(source, sandbox, { filename: 'sw.js' });

    return {
        showNotificationCalls,
        setWindowClients(list: MockWindowClient[]) {
            matchAllResult = list;
        },
        getOpenedWindowUrl(): string | null {
            return openedWindowUrl;
        },
        async dispatchPush(eventData: { json(): unknown } | null): Promise<void> {
            let waited: Promise<unknown> | undefined;
            const event = {
                data: eventData,
                waitUntil(p: Promise<unknown>) {
                    waited = p;
                },
            };
            handlers.push?.(event);
            if (waited) await waited;
        },
        async dispatchNotificationClick(notification: { close: () => void }): Promise<void> {
            let waited: Promise<unknown> | undefined;
            const event = {
                notification,
                waitUntil(p: Promise<unknown>) {
                    waited = p;
                },
            };
            handlers.notificationclick?.(event);
            if (waited) await waited;
        },
    };
}

test.describe('sw.js push notifications (#264)', () => {
    test('push event shows a notification with the payload title/body', async () => {
        const sw = loadSW();
        await sw.dispatchPush({
            json: () => ({ title: 'Dochadza ti kredit', body: 'Zostatok 1.00 EUR.' }),
        });
        expect(sw.showNotificationCalls).toHaveLength(1);
        expect(sw.showNotificationCalls[0].title).toBe('Dochadza ti kredit');
        expect(sw.showNotificationCalls[0].options.body).toBe('Zostatok 1.00 EUR.');
    });

    test('push event with no data falls back to a generic notification instead of throwing', async () => {
        const sw = loadSW();
        await sw.dispatchPush(null);
        expect(sw.showNotificationCalls).toHaveLength(1);
        expect(sw.showNotificationCalls[0].title).toBe('SpinBike');
        expect(sw.showNotificationCalls[0].options.body).toBe('');
    });

    test('push event whose data.json() throws falls back instead of throwing', async () => {
        const sw = loadSW();
        await sw.dispatchPush({
            json: () => {
                throw new Error('malformed');
            },
        });
        expect(sw.showNotificationCalls).toHaveLength(1);
        expect(sw.showNotificationCalls[0].title).toBe('SpinBike');
    });

    test('notificationclick focuses an already-open /my/balance tab instead of opening a new one', async () => {
        const sw = loadSW();
        let closed = false;
        let focused = false;
        const client: MockWindowClient = {
            url: 'https://spinbike.sk/my/balance',
            focus() {
                focused = true;
                return Promise.resolve(this);
            },
        };
        sw.setWindowClients([client]);

        await sw.dispatchNotificationClick({
            close: () => {
                closed = true;
            },
        });

        expect(closed).toBe(true);
        expect(focused).toBe(true);
        expect(sw.getOpenedWindowUrl()).toBeNull();
    });

    test('notificationclick opens a new /my/balance tab when none is open', async () => {
        const sw = loadSW();
        sw.setWindowClients([]);

        await sw.dispatchNotificationClick({ close: () => {} });

        expect(sw.getOpenedWindowUrl()).toBe('/my/balance');
    });
});
