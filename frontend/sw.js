// YEET PWA Service Worker v1
const CACHE = 'yeet-v1';
const STATIC = [
  '/',
  '/manifest.json',
];

// Install — cache shell
self.addEventListener('install', e => {
  e.waitUntil(
    caches.open(CACHE).then(c => c.addAll(STATIC))  // skipWaiting disabled to prevent forced reload
  );
});

// Activate — clean old caches
self.addEventListener('activate', e => {
  e.waitUntil(
    caches.keys().then(keys =>
      Promise.all(keys.filter(k => k !== CACHE).map(k => caches.delete(k)))
    )  // clients.claim disabled to prevent forced reload
  );
});

// Fetch strategy:
// - API calls: network-first, no cache
// - Static assets: cache-first
// - HTML shell: network-first with offline fallback
self.addEventListener('fetch', e => {
  const url = new URL(e.request.url);

  // API: always network, never cache
  if (url.pathname.startsWith('/api/')) {
    e.respondWith(fetch(e.request).catch(() =>
      new Response(JSON.stringify({error:{code:'OFFLINE',message:'No network connection'}}),
        {status: 503, headers:{'Content-Type':'application/json'}})
    ));
    return;
  }

  // Fonts/external: network only
  if (url.origin !== location.origin) {
    e.respondWith(fetch(e.request).catch(() => new Response('', {status: 408})));
    return;
  }

  // HTML: network-first with cache fallback
  if (e.request.mode === 'navigate') {
    e.respondWith(
      fetch(e.request)
        .then(res => { caches.open(CACHE).then(c => c.put(e.request, res.clone())); return res; })
        .catch(() => caches.match('/') || caches.match(e.request))
    );
    return;
  }

  // Static assets: cache-first
  e.respondWith(
    caches.match(e.request).then(cached => {
      if (cached) return cached;
      return fetch(e.request).then(res => {
        if (res.ok) caches.open(CACHE).then(c => c.put(e.request, res.clone()));
        return res;
      });
    })
  );
});

// Push notifications (future)
self.addEventListener('push', e => {
  const data = e.data?.json() ?? {title: 'YEET', body: 'New notification'};
  e.waitUntil(
    self.registration.showNotification(data.title, {
      body: data.body,
      icon: '/icons/icon-192.png',
      badge: '/icons/icon-96.png',
      data: data.url ? {url: data.url} : undefined,
    })
  );
});

self.addEventListener('notificationclick', e => {
  e.notification.close();
  if (e.notification.data?.url) {
    e.waitUntil(clients.openWindow(e.notification.data.url));
  }
});