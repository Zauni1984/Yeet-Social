// Service Worker disabled - no caching, no reload
self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', e => e.waitUntil(
  caches.keys().then(keys => Promise.all(keys.map(k => caches.delete(k))))
));
self.addEventListener('fetch', e => e.respondWith(fetch(e.request)));
