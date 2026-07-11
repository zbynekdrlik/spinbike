import { test, expect } from '@playwright/test';
import { readFileSync } from 'fs';
import * as path from 'path';
import vm from 'vm';

/**
 * Regression coverage for #208 — the service worker cache-first-pinned every
 * non-root SPA route's HTML forever, so a fresh deploy (new content-hashed
 * asset filenames baked into the server binary) would leave any user who had
 * bookmarked `/login` / `/dashboard` / `/my/balance` stuck on a stale bundle —
 * or outright broken once the old hashed asset paths 404 against the new binary.
 *
 * The old `isVolatile(url)` only network-first'd `/`, `*.html`, `/sw.js`,
 * `/manifest.json`; every extension-less SPA route fell into the cache-first
 * branch and got pinned.
 *
 * We test the REAL shipped `spinbike-ui/sw.js` (not a copy) by loading it into
 * a mocked ServiceWorkerGlobalScope (`self`, `caches`, `fetch`) via `vm` and
 * driving synthetic FetchEvents. This is deterministic and server-independent
 * (a real-browser SW test cannot force a "new deploy" mid-run, per the sw
 * fetch/cache constraints), yet asserts real behaviour that FAILS on the old
 * URL-shape heuristic: a second fetch of an SPA route across a simulated deploy
 * must return the FRESH content, not the stale cached copy.
 */

const SW_PATH = path.join(__dirname, '..', '..', 'spinbike-ui', 'sw.js');
const ORIGIN = 'https://spinbike.sk';

type NetworkResponder = (request: { url: string }) => Response;

interface MockCaches {
    open(name: string): Promise<{
        put(request: { url: string }, response: Response): Promise<void>;
        match(request: { url: string }): Promise<Response | undefined>;
        keys(): Promise<string[]>;
    }>;
    match(request: { url: string }): Promise<Response | undefined>;
    keys(): Promise<string[]>;
    delete(name: string): Promise<boolean>;
    lastOpenedName(): string | null;
    seed(name: string, url: string, response: Response): void;
    hasCache(name: string): boolean;
}

interface LoadedSW {
    cacheName: string;
    mockCaches: MockCaches;
    setNetwork(responder: NetworkResponder): void;
    setOffline(): void;
    dispatchFetch(url: string): Promise<Response | undefined>;
    dispatchActivate(): Promise<void>;
}

const html = (body: string): Response =>
    new Response(body, { status: 200, headers: { 'content-type': 'text/html; charset=utf-8' } });
const js = (body: string): Response =>
    new Response(body, { status: 200, headers: { 'content-type': 'application/javascript' } });

// Drain the microtask queue AND one macrotask tick, so the service worker's
// fire-and-forget `caches.open(...).then(cache.put(...))` (not awaited before
// the response is returned) has definitely settled before the next dispatch.
const flush = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0));

function makeMockCaches(): MockCaches {
    const store = new Map<string, Map<string, Response>>();
    let lastOpened: string | null = null;
    const cacheFor = (name: string): Map<string, Response> => {
        let c = store.get(name);
        if (!c) {
            c = new Map();
            store.set(name, c);
        }
        return c;
    };
    return {
        open(name: string) {
            lastOpened = name;
            return Promise.resolve({
                put: (request: { url: string }, response: Response) => {
                    cacheFor(name).set(request.url, response);
                    return Promise.resolve();
                },
                match: (request: { url: string }) => {
                    const r = cacheFor(name).get(request.url);
                    return Promise.resolve(r ? r.clone() : undefined);
                },
                keys: () => Promise.resolve([...cacheFor(name).keys()]),
            });
        },
        match(request: { url: string }) {
            for (const c of store.values()) {
                const r = c.get(request.url);
                if (r) return Promise.resolve(r.clone());
            }
            return Promise.resolve(undefined);
        },
        keys: () => Promise.resolve([...store.keys()]),
        delete: (name: string) => Promise.resolve(store.delete(name)),
        lastOpenedName: () => lastOpened,
        seed: (name: string, url: string, response: Response) => {
            cacheFor(name).set(url, response);
        },
        hasCache: (name: string) => store.has(name),
    };
}

function loadServiceWorker(): LoadedSW {
    const source = readFileSync(SW_PATH, 'utf-8');
    const mockCaches = makeMockCaches();

    let responder: NetworkResponder | null = null;
    let offline = false;
    const mockFetch = (request: { url: string }): Promise<Response> =>
        Promise.resolve().then(() => {
            if (offline) throw new Error('offline');
            if (!responder) throw new Error('no network responder configured for this test');
            return responder(request);
        });

    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const handlers: Record<string, (event: any) => void> = {};
    const self = {
        addEventListener(type: string, fn: (event: unknown) => void) {
            handlers[type] = fn;
        },
        skipWaiting() {},
        clients: { claim() {} },
    };

    const sandbox: Record<string, unknown> = {
        self,
        caches: mockCaches,
        fetch: mockFetch,
        URL,
        Promise,
        console,
    };
    vm.createContext(sandbox);
    vm.runInContext(source, sandbox, { filename: 'sw.js' });

    // The install handler runs `caches.open(CACHE_NAME)` synchronously, which
    // records the name — the only way to read the module-scoped `const`.
    handlers.install?.({ waitUntil() {} });
    const cacheName = mockCaches.lastOpenedName();
    expect(cacheName, 'install handler must open the cache (captures CACHE_NAME)').toBeTruthy();

    return {
        cacheName: cacheName as string,
        mockCaches,
        setNetwork: (r: NetworkResponder) => {
            responder = r;
            offline = false;
        },
        setOffline: () => {
            offline = true;
        },
        async dispatchFetch(url: string): Promise<Response | undefined> {
            let responded: Promise<Response> | undefined;
            const event = {
                request: { url },
                respondWith(p: Response | Promise<Response>) {
                    responded = Promise.resolve(p);
                },
            };
            handlers.fetch?.(event);
            await flush();
            return responded ? await responded : undefined;
        },
        async dispatchActivate(): Promise<void> {
            let waited: Promise<unknown> | undefined;
            handlers.activate?.({
                waitUntil(p: Promise<unknown>) {
                    waited = p;
                },
            });
            if (waited) await waited;
            await flush();
        },
    };
}

test.describe('Service worker caching strategy (#208)', () => {
    test('SPA route HTML is network-first: a new deploy is picked up, not pinned', async () => {
        const sw = loadServiceWorker();

        // Deploy N: first visit to a bookmarked SPA route.
        sw.setNetwork(() => html('<html>DEPLOY-OLD</html>'));
        const first = await sw.dispatchFetch(`${ORIGIN}/login`);
        expect(await first!.text()).toContain('DEPLOY-OLD');

        // Deploy N+1: the server now serves fresh HTML (new hashed asset refs).
        sw.setNetwork(() => html('<html>DEPLOY-NEW</html>'));
        const second = await sw.dispatchFetch(`${ORIGIN}/login`);

        // The whole bug: on the old URL-shape isVolatile(), `/login` was
        // cache-first, so this second fetch returned the STALE 'DEPLOY-OLD'
        // pinned copy. It must now return the fresh network content.
        expect(await second!.text()).toContain('DEPLOY-NEW');
    });

    test('the same pinning bug is gone for every non-root route shape', async () => {
        for (const route of ['/dashboard', '/my/balance', '/welcome', '/staff']) {
            const sw = loadServiceWorker();
            sw.setNetwork(() => html(`OLD ${route}`));
            const first = await sw.dispatchFetch(`${ORIGIN}${route}`);
            expect(await first!.text()).toContain(`OLD ${route}`);

            sw.setNetwork(() => html(`NEW ${route}`));
            const second = await sw.dispatchFetch(`${ORIGIN}${route}`);
            expect(await second!.text(), `${route} must refresh, not pin`).toContain(`NEW ${route}`);
        }
    });

    test('content-hashed assets under /assets/ stay cache-first (offline/perf preserved)', async () => {
        const sw = loadServiceWorker();
        const asset = `${ORIGIN}/assets/spinbike-ui-abc123.js`;

        sw.setNetwork(() => js('ASSET_V1'));
        const first = await sw.dispatchFetch(asset);
        expect(await first!.text()).toBe('ASSET_V1');

        // Even if the network changes, a cache-first asset keeps serving the
        // cached (immutable) copy — a new deploy gets a NEW hashed filename,
        // so serving the cached one for THIS url forever is correct + fast.
        sw.setNetwork(() => js('ASSET_V2'));
        const second = await sw.dispatchFetch(asset);
        expect(await second!.text(), 'hashed asset must be served from cache').toBe('ASSET_V1');
    });

    test('offline falls back to the cached HTML copy (PWA offline preserved)', async () => {
        const sw = loadServiceWorker();

        sw.setNetwork(() => html('SHELL'));
        await sw.dispatchFetch(`${ORIGIN}/dashboard`);

        sw.setOffline();
        const offlineResp = await sw.dispatchFetch(`${ORIGIN}/dashboard`);
        expect(offlineResp, 'must serve something offline').toBeTruthy();
        expect(await offlineResp!.text()).toContain('SHELL');
    });

    test('/api/* and /ws* bypass the service worker entirely', async () => {
        const sw = loadServiceWorker();
        sw.setNetwork(() => html('should-not-be-used'));

        const api = await sw.dispatchFetch(`${ORIGIN}/api/version`);
        expect(api, '/api/* must not be intercepted (respondWith not called)').toBeUndefined();

        const ws = await sw.dispatchFetch(`${ORIGIN}/ws`);
        expect(ws, '/ws* must not be intercepted').toBeUndefined();
    });

    test('CACHE_NAME has moved past the corrupt v2 generation, and activate purges stale caches', async () => {
        const sw = loadServiceWorker();

        // The current bug corrupted the `spinbike-v2` cache with pinned SPA
        // HTML. Shipping the fix MUST bump CACHE_NAME so activate purges those
        // poisoned per-route caches on every existing user's next visit.
        expect(sw.cacheName, 'must not reuse the corrupt spinbike-v2 cache generation').not.toBe(
            'spinbike-v2',
        );

        sw.mockCaches.seed('spinbike-v2', `${ORIGIN}/login`, html('poisoned'));
        sw.mockCaches.seed(sw.cacheName, `${ORIGIN}/assets/app.js`, js('current'));

        await sw.dispatchActivate();

        expect(sw.mockCaches.hasCache('spinbike-v2'), 'stale cache must be deleted').toBe(false);
        expect(sw.mockCaches.hasCache(sw.cacheName), 'current cache must survive').toBe(true);
    });
});
