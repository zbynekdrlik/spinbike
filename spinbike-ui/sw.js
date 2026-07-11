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
// documents — the app shell AND every extension-less SPA route (/login,
// /dashboard, /my/balance, ...) — embed Trunk's content-hashed JS/WASM/CSS
// filenames, which change on every deploy. Caching an HTML doc first pins the
// user to a stale bundle whose asset URLs 404 after the next deploy (#208).
// Content-Type — not URL shape — is the reliable signal, since SPA routes have
// no file extension and the old URL-path heuristic silently missed them.
function isHtml(resp) {
    const ct = (resp.headers.get('content-type') || '').toLowerCase();
    return ct.startsWith('text/html');
}

// Cache-first for content-hashed immutable assets (fast, offline-capable). A
// new deploy always produces a NEW hashed filename, so serving a cached copy of
// THIS url forever is correct. The isHtml guard means that even if a stray
// /assets/* miss falls through to the server's SPA index.html fallback, that
// HTML is never pinned.
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

// Network-first for everything else (HTML shell + SPA routes): always try the
// network so a fresh deploy is picked up immediately; keep a copy for offline
// fallback; serve the cached copy only when the network is unreachable.
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

    // Content-hashed immutable assets (Trunk writes them under /assets/ and the
    // server marks them `immutable`) are safe to serve cache-first. New SPA
    // routes need no change here — they fall into the network-first branch
    // automatically, so freshness self-adapts without editing this file.
    if (url.pathname.startsWith('/assets/')) {
        event.respondWith(cacheFirst(event.request));
        return;
    }

    event.respondWith(networkFirst(event.request));
});
