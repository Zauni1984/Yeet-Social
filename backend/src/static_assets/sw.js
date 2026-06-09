// Yeet Social — service worker for tickle pushes.
//
// We only handle the "push" event (and the "notificationclick"
// follow-up). The payload is always empty: the server intentionally
// sends content-less pushes so the push service (FCM, Mozilla
// autopush, Apple) never sees ciphertext or hint metadata. The user
// taps the notification to come back and decrypt locally.
//
// Generic notification copy is used by design — no conversation name,
// no sender. Anything more specific would have to round-trip through
// the push payload, which we've decided not to do.

self.addEventListener('install', function(event){
  event.waitUntil(self.skipWaiting());
});

self.addEventListener('activate', function(event){
  event.waitUntil(self.clients.claim());
});

self.addEventListener('push', function(event){
  var title = 'New message';
  var options = {
    body: 'You have a new message on Yeet Social.',
    icon: '/favicon.png',
    badge: '/favicon.png',
    tag: 'yeet-new-message',
    renotify: false,
    data: { url: '/' }
  };
  event.waitUntil(self.registration.showNotification(title, options));
});

self.addEventListener('notificationclick', function(event){
  event.notification.close();
  var target = (event.notification.data && event.notification.data.url) || '/';
  event.waitUntil(
    self.clients.matchAll({ type:'window', includeUncontrolled:true })
      .then(function(list){
        for(var i=0;i<list.length;i++){
          var c = list[i];
          if('focus' in c){
            try { c.navigate(target); } catch(e){}
            return c.focus();
          }
        }
        if(self.clients.openWindow) return self.clients.openWindow(target);
      })
  );
});
