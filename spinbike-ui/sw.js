// Bump CACHE_NAME on any breaking change to invalidate stale browser caches.
const CACHE_NAME = 'spinbike-v2';

// Paths whose content changes on every deploy (HTML + the service worker
// itself). These must ALWAYS fetch fresh — caching them causes the page to
// reference a hashed WASM bundle that no longer exists after a deploy.
function isVolatile(url) {
    const p = url.pathname;
    if (p === '/' || p.endsWith('.html') || p === '/sw.js' || p === '/manifest.json') {
        return true;
    }
    // Trunk writes hashed asset filenames (e.g. spinbike-ui-<hash>.js).
    // Anything with a fingerprint-looking name is immutable, safe to cache.
    return false;
}

self.addEventListener('install', (event) => {
    event.waitUntil(caches.open(CACHE_NAME));
    self.skipWaiting();
});

self.addEventListener('activate', (event) => {
    event.waitUntil(
        caches
            .keys()
            .then((keys) =>
                Promise.all(keys.filter((k) => k !== CACHE_NAME).map((k) => caches.delete(k)))
            )
    );
    self.clients.claim();
});

self.addEventListener('fetch', (event) => {
    const url = new URL(event.request.url);

    // Never cache API or WebSocket traffic.
    if (url.pathname.startsWith('/api/') || url.pathname.startsWith('/ws')) {
        return;
    }

    // Network-first for volatile files: always try network, fall back to
    // cache only if offline. This ensures a fresh deploy is picked up on the
    // next page load, not stuck behind a stale-while-revalidate loop.
    if (isVolatile(url)) {
        event.respondWith(
            fetch(event.request)
                .then((resp) => {
                    if (resp.ok) {
                        const clone = resp.clone();
                        caches.open(CACHE_NAME).then((cache) => cache.put(event.request, clone));
                    }
                    return resp;
                })
                .catch(() => caches.match(event.request))
        );
        return;
    }

    // Cache-first for hashed/immutable assets. Falls back to network + populates cache.
    event.respondWith(
        caches.match(event.request).then(
            (cached) =>
                cached ||
                fetch(event.request).then((resp) => {
                    if (resp.ok) {
                        const clone = resp.clone();
                        caches.open(CACHE_NAME).then((cache) => cache.put(event.request, clone));
                    }
                    return resp;
                })
        )
    );
});
