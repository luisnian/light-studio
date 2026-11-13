// Service worker: la app abre sin cobertura. Nunca cachea la API.
const SHELL = 'pdi-shell-v21';
const TILES = 'pdi-tiles-v2';
const MAX_TILES = 1500;
const FILES = ['./', 'index.html', 'app.js', 'style.css', 'manifest.webmanifest', 'vendor/leaflet.js', 'vendor/leaflet.css',
  'vendor/images/marker-icon.png', 'vendor/images/layers.png', 'icons/icon-192.png', 'icons/favicon-32.png', 'icons/favicon.svg', 'icons/apple-touch-icon.png'];

self.addEventListener('install', (e) => {
  e.waitUntil(caches.open(SHELL).then((c) => c.addAll(FILES)).then(() => self.skipWaiting()));
});
self.addEventListener('activate', (e) => {
  e.waitUntil(caches.keys().then((ks) => Promise.all(ks.filter((k) => ![SHELL, TILES].includes(k)).map((k) => caches.delete(k)))).then(() => self.clients.claim()));
});

async function trim(cache) {
  const keys = await cache.keys();
  for (let i = 0; i < keys.length - MAX_TILES; i++) await cache.delete(keys[i]);
}

self.addEventListener('fetch', (e) => {
  const req = e.request;
  if (req.method !== 'GET') return;
  const url = new URL(req.url);
  if (url.origin === location.origin && url.pathname.startsWith('/api/')) return; // API: directo a la red

  if (url.hostname.endsWith('tile.openstreetmap.org') || url.hostname === 'server.arcgisonline.com') { // teselas vistas: se guardan para verlas sin cobertura
    e.respondWith(caches.open(TILES).then(async (c) => {
      const hit = await c.match(req);
      if (hit) return hit;
      try { const r = await fetch(req); if (r.ok) { c.put(req, r.clone()); trim(c); } return r; } catch { return new Response('', { status: 504 }); }
    }));
    return;
  }
  if (url.origin !== location.origin) return;

  // Resto: red primero (para recibir actualizaciones) y caché si no hay cobertura
  e.respondWith(fetch(req).then((r) => { if (r.ok) { const cp = r.clone(); caches.open(SHELL).then((c) => c.put(req, cp)); } return r; })
    .catch(() => caches.match(req, { ignoreSearch: true }).then((h) => h || caches.match('index.html'))));
});
