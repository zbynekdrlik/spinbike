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
 * The fix distinguishes HTML document navigations from static subresources by
 * `request.mode === 'navigate'` (the canonical service-worker discriminator,
 * robust across every route AND every asset URL — this app's Trunk bundle is
 * served at the ROOT, e.g. `/spinbike-ui-<hash>.js` / `_bg.wasm`, NOT under
 * `/assets/`, so a URL-prefix heuristic would wrongly drop the 2.4 MB WASM out
 * of cache-first). Navigations are network-first (always fresh, never pinned);
 * everything else is cache-first (fast, offline, immutable-per-deploy).
 *
 * We test the REAL shipped `spinbike-ui/sw.js` by loading it into a mocked
 * ServiceWorkerGlobalScope (`self`, `caches`, `fetch`) via `vm` and driving
 * synthetic FetchEvents — deterministic and server-independent (a real-browser
 * SW test cannot force a "new deploy" mid-run). Assertions FAIL on the old
 * URL-shape heuristic (SPA routes pinned) AND on a naive `/assets/`-only rewrite
 * (the root-level WASM/JS dropping out of cache-first).
 */

const SW_PATH = path.join(__dirname, '..', '..', 'spinbike-ui', 'sw.js');
const ORIGIN = 'https://spinbike.sk';

type MockRequest = { url: string; mode: string };
type NetworkResponder = (request: MockRequest) => Response;

interface MockCaches {
    open(name: string): Promise<{
        put(request: MockRequest, response: Response): Promise<void>;
        match(request: MockRequest): Promise<Response | undefined>;
        keys(): Promise<string[]>;
    }>;
    match(request: MockRequest): Promise<Response | undefined>;
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
    dispatchFetch(url: string, mode: string): Promise<Response | undefined>;
    dispatchActivate(): Promise<void>;
}

const html = (body: string): Response =>
    new Response(body, { status: 200, headers: { 'content-type': 'text/html; charset=utf-8' } });
const js = (body: string): Response =>
    new Response(body, { status: 200, headers: { 'content-type': 'text/javascript' } });

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
                put: (request: MockRequest, response: Response) => {
                    cacheFor(name).set(request.url, response);
                    return Promise.resolve();
                },
                match: (request: MockRequest) => {
                    const r = cacheFor(name).get(request.url);
                    return Promise.resolve(r ? r.clone() : undefined);
                },
                keys: () => Promise.resolve([...cacheFor(name).keys()]),
            });
        },
        match(request: MockRequest) {
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
    const mockFetch = (request: MockRequest): Promise<Response> =>
        Promise.resolve().then(() => {
            if (offline) throw new Error('offline');
            if (!responder) throw new Error('no network responder configured for this test');
            return responder(request);
        });

    const handlers: Record<string, (event: unknown) => void> = {};
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
        async dispatchFetch(url: string, mode: string): Promise<Response | undefined> {
            let responded: Promise<Response> | undefined;
            const event = {
                request: { url, mode },
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
    test('SPA route navigation is network-first: a new deploy is picked up, not pinned', async () => {
        const sw = loadServiceWorker();

        // Deploy N: first hard-load of a bookmarked SPA route (mode 'navigate').
        sw.setNetwork(() => html('<html>DEPLOY-OLD</html>'));
        const first = await sw.dispatchFetch(`${ORIGIN}/login`, 'navigate');
        expect(await first!.text()).toContain('DEPLOY-OLD');

        // Deploy N+1: the server now serves fresh HTML (new hashed asset refs).
        sw.setNetwork(() => html('<html>DEPLOY-NEW</html>'));
        const second = await sw.dispatchFetch(`${ORIGIN}/login`, 'navigate');

        // The whole bug: on the old URL-shape isVolatile(), `/login` was
        // cache-first, so this second fetch returned the STALE 'DEPLOY-OLD'
        // pinned copy. It must now return the fresh network content.
        expect(await second!.text()).toContain('DEPLOY-NEW');
    });

    test('the same pinning bug is gone for every non-root route', async () => {
        for (const route of ['/dashboard', '/my/balance', '/welcome', '/staff']) {
            const sw = loadServiceWorker();
            sw.setNetwork(() => html(`OLD ${route}`));
            const first = await sw.dispatchFetch(`${ORIGIN}${route}`, 'navigate');
            expect(await first!.text()).toContain(`OLD ${route}`);

            sw.setNetwork(() => html(`NEW ${route}`));
            const second = await sw.dispatchFetch(`${ORIGIN}${route}`, 'navigate');
            expect(await second!.text(), `${route} must refresh, not pin`).toContain(`NEW ${route}`);
        }
    });

    test('the root-level content-hashed bundle stays cache-first (offline/perf preserved)', async () => {
        const sw = loadServiceWorker();
        // Trunk serves the bundle at the ROOT, not under /assets/ — the 2.4 MB
        // WASM has no cache-control header, so SW cache-first is what keeps hard
        // navigations from re-downloading it. A subresource fetch is NOT mode
        // 'navigate'.
        const bundle = `${ORIGIN}/spinbike-ui-a6d6074bb59d2b0a.js`;

        sw.setNetwork(() => js('BUNDLE_V1'));
        const first = await sw.dispatchFetch(bundle, 'cors');
        expect(await first!.text()).toBe('BUNDLE_V1');

        // Even if the network changes, a cache-first immutable asset keeps
        // serving the cached copy for THIS url (a new deploy gets a NEW hash).
        // A naive `/assets/`-only rewrite would have made this root-level bundle
        // network-first and returned BUNDLE_V2 here — the regression this guards.
        sw.setNetwork(() => js('BUNDLE_V2'));
        const second = await sw.dispatchFetch(bundle, 'cors');
        expect(await second!.text(), 'root-level hashed bundle must be served from cache').toBe(
            'BUNDLE_V1',
        );
    });

    test('a non-navigate request that returns HTML is never cache-first-pinned', async () => {
        // Defence-in-depth: if a subresource path (no mode 'navigate') misses on
        // the server and falls through to the SPA index.html fallback, the
        // content-type guard must keep it out of the cache-first store.
        const sw = loadServiceWorker();
        const strayPath = `${ORIGIN}/some/stray/subresource`;

        sw.setNetwork(() => html('STRAY-OLD'));
        await sw.dispatchFetch(strayPath, 'cors');

        sw.setNetwork(() => html('STRAY-NEW'));
        const second = await sw.dispatchFetch(strayPath, 'cors');
        expect(await second!.text(), 'HTML response must not be pinned even off a subresource path').toContain(
            'STRAY-NEW',
        );
    });

    test('offline falls back to the cached HTML copy (PWA offline preserved)', async () => {
        const sw = loadServiceWorker();

        sw.setNetwork(() => html('SHELL'));
        await sw.dispatchFetch(`${ORIGIN}/dashboard`, 'navigate');

        sw.setOffline();
        const offlineResp = await sw.dispatchFetch(`${ORIGIN}/dashboard`, 'navigate');
        expect(offlineResp, 'must serve something offline').toBeTruthy();
        expect(await offlineResp!.text()).toContain('SHELL');
    });

    test('/api/* and /ws* bypass the service worker entirely', async () => {
        const sw = loadServiceWorker();
        sw.setNetwork(() => html('should-not-be-used'));

        const api = await sw.dispatchFetch(`${ORIGIN}/api/version`, 'cors');
        expect(api, '/api/* must not be intercepted (respondWith not called)').toBeUndefined();

        const ws = await sw.dispatchFetch(`${ORIGIN}/ws`, 'websocket');
        expect(ws, '/ws* must not be intercepted').toBeUndefined();
    });

    test('CACHE_NAME has moved past the corrupt v2 generation, and activate purges stale caches', async () => {
        const sw = loadServiceWorker();

        // The original bug corrupted the `spinbike-v2` cache with pinned SPA
        // HTML. Shipping the fix MUST bump CACHE_NAME so activate purges those
        // poisoned per-route caches on every existing user's next visit.
        expect(sw.cacheName, 'must not reuse the corrupt spinbike-v2 cache generation').not.toBe(
            'spinbike-v2',
        );

        sw.mockCaches.seed('spinbike-v2', `${ORIGIN}/login`, html('poisoned'));
        sw.mockCaches.seed(sw.cacheName, `${ORIGIN}/spinbike-ui-abc.js`, js('current'));

        await sw.dispatchActivate();

        expect(sw.mockCaches.hasCache('spinbike-v2'), 'stale cache must be deleted').toBe(false);
        expect(sw.mockCaches.hasCache(sw.cacheName), 'current cache must survive').toBe(true);
    });
});
