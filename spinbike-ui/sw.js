// Bump CACHE_NAME on any breaking change to invalidate stale browser caches.
// v3 (#208): the previous strategy (v2) cache-first-pinned every non-root SPA
// route's HTML forever. Bumping purges those poisoned per-route caches on the
// next visit via the activate handler below.
const CACHE_NAME = 'spinbike-v3';

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

// Only genuinely static (non-HTML) responses may be cached-first. HTML
// documents change their embedded content-hashed asset references on every
// deploy, so pinning one leaves the user on a stale bundle whose asset URLs
// 404 after the next deploy (#208). The content-type guard is defence-in-depth
// behind the navigation-mode routing below.
function isHtml(resp) {
    const ct = (resp.headers.get('content-type') || '').toLowerCase();
    return ct.startsWith('text/html');
}

// Cache-first for immutable subresources (the content-hashed JS/WASM bundle,
// CSS, fonts, icons). A new deploy always produces a NEW hashed filename, so
// serving a cached copy of THIS url forever is correct + fast. The isHtml guard
// means a stray HTML SPA-fallback (e.g. a mistyped subresource path the server
// answers with index.html) is never pinned.
function cacheFirst(request) {
    return caches.match(request).then((cached) => {
        if (cached) {
            return cached;
        }
        return fetch(request).then((resp) => {
            if (resp.ok && !isHtml(resp)) {
                const clone = resp.clone();
                caches.open(CACHE_NAME).then((cache) => cache.put(request, clone));
            }
            return resp;
        });
    });
}

// Network-first for HTML document navigations (the app shell + every SPA route):
// always try the network so a fresh deploy is picked up immediately; keep a copy
// for offline fallback; serve the cached copy only when the network is
// unreachable.
function networkFirst(request) {
    return fetch(request)
        .then((resp) => {
            if (resp.ok) {
                const clone = resp.clone();
                caches.open(CACHE_NAME).then((cache) => cache.put(request, clone));
            }
            return resp;
        })
        .catch(() =>
            caches.match(request).then((cached) => {
                if (cached) {
                    return cached;
                }
                throw new Error('offline and no cached copy');
            })
        );
}

self.addEventListener('fetch', (event) => {
    const url = new URL(event.request.url);

    // Never cache API or WebSocket traffic.
    if (url.pathname.startsWith('/api/') || url.pathname.startsWith('/ws')) {
        return;
    }

    // HTML document navigations (root, /login, /dashboard, /my/balance, ...) are
    // identified by request.mode === 'navigate' — the canonical service-worker
    // discriminator. This self-adapts to ANY route with no URL list, and (unlike
    // a /assets/ or extension heuristic) correctly treats this app's ROOT-served
    // Trunk bundle (/spinbike-ui-<hash>.js, _bg.wasm) as a cacheable subresource
    // rather than a navigation.
    if (event.request.mode === 'navigate') {
        event.respondWith(networkFirst(event.request));
        return;
    }

    event.respondWith(cacheFirst(event.request));
});
