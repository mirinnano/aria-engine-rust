const CACHE = "aria-v3-shell-1.0.0";
const RUNTIME = __ARIA_WEB_RUNTIME_CACHE__;
const SHELL = [
  "./",
  "./index.html",
  "./app.css",
  "./main.js",
  "./web-audio.js",
  "./web-renderer.js",
  "./save-store.js",
  "./manifest.webmanifest",
  "./build-manifest.json",
  "./bundle.aria.json",
  "./game.ariac",
  "./game.ariapak",
  ...RUNTIME,
];

self.addEventListener("install", (event) => {
  event.waitUntil(caches.open(CACHE).then((cache) => cache.addAll(SHELL)));
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((key) => key !== CACHE).map((key) => caches.delete(key))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("message", (event) => {
  if (event.data?.type === "SKIP_WAITING") self.skipWaiting();
});

self.addEventListener("fetch", (event) => {
  if (event.request.method !== "GET") return;
  const url = new URL(event.request.url);
  if (url.origin !== self.location.origin) return;
  event.respondWith(
    caches.match(event.request).then(async (cached) => {
      if (cached) return cached;
      try {
        const response = await fetch(event.request);
        if (response.ok) {
          const cache = await caches.open(CACHE);
          cache.put(event.request, response.clone());
        }
        return response;
      } catch (error) {
        if (event.request.mode === "navigate") return caches.match("./index.html");
        throw error;
      }
    }),
  );
});
