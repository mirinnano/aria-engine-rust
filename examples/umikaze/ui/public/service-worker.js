// These files are a single runtime contract.  They intentionally retain
// stable names for desktop packaging, so a cache-first response can otherwise
// combine a newer React shell with an older WASM VM (or the reverse) after an
// update.  That was the source of the blank startup screen seen in 3.2.0.
// Contract assets are network-first, so a stable cache name can be safely
// reused across releases while still replacing the shell atomically. A new
// service worker also removes the legacy versioned caches on activation.
const CACHE = "umikaze-shell";
const CONTRACT_FILENAMES = new Set([
  "index.html",
  "bundle.aria.json",
  "game.ariac",
  "game.ariapak",
  "game.hot.ariapak",
  "game.cold.ariapak",
  "game.overlay.ariapak",
  "aria_web.js",
  "aria_web_bg.wasm",
  "web-renderer.js",
  "web-audio.js",
  "save-store.js",
]);

function isContractAsset(url) {
  const segments = url.pathname.split("/");
  return CONTRACT_FILENAMES.has(segments[segments.length - 1] || "");
}

self.addEventListener("install", (event) => {
  event.waitUntil(caches.open(CACHE).then((cache) => cache.add("./")));
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches.keys().then((keys) => Promise.all(keys
      .filter((key) => key !== CACHE)
      .map((key) => caches.delete(key))))
      .then(() => self.clients.claim()),
  );
});

async function cacheResponse(request, response) {
  if (response.ok) {
    const cache = await caches.open(CACHE);
    await cache.put(request, response.clone());
  }
  return response;
}

async function networkFirst(request) {
  try {
    return await cacheResponse(request, await fetch(request));
  } catch {
    const cached = await caches.match(request);
    if (cached) return cached;
    throw new Error("The record is unavailable offline.");
  }
}

// Cache game data and hashed Vite assets lazily.  Contract assets always go
// to the network first so an update is atomically coherent whenever the
// network is available; the previous generation remains the offline fallback.
self.addEventListener("fetch", (event) => {
  if (event.request.method !== "GET") return;
  const url = new URL(event.request.url);
  if (url.origin !== self.location.origin) return;
  if (event.request.mode === "navigate" || isContractAsset(url)) {
    event.respondWith(networkFirst(event.request));
    return;
  }
  event.respondWith(caches.match(event.request).then((cached) => cached || fetch(event.request)
    .then((response) => cacheResponse(event.request, response))));
});
