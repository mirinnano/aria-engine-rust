// The package builder derives this identifier from every staged game and
// shell file. A later deployment therefore never combines a cached older PAK
// with a newer bytecode bundle at the same public URL.
const CACHE = "aria-v3-shell-__ARIA_WEB_CACHE_ID__";
// The package builder expands this into the lightweight files that were
// actually staged. PAK roles are optional and large, so payloads are cached
// lazily by the fetch handler after use rather than racing a second download
// while the first page is still booting.
const SHELL = __ARIA_WEB_SHELL__;

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
