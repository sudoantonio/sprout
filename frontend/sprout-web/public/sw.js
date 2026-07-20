const CACHE_VERSION = 'sprout-shell-v2'
const SHELL_URLS = ['/', '/manifest.webmanifest', '/app-icon.svg', '/favicon.svg']

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches
      .open(CACHE_VERSION)
      .then((cache) => cache.addAll(SHELL_URLS))
      .then(() => self.skipWaiting()),
  )
})

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(
          keys
            .filter((key) => key !== CACHE_VERSION)
            .map((key) => caches.delete(key)),
        ),
      )
      .then(() => self.clients.claim()),
  )
})

function isCacheableShellAsset(request, url) {
  if (request.method !== 'GET' || url.origin !== self.location.origin) {
    return false
  }

  // Only generated application assets and fixed PWA artwork enter Cache Storage.
  // API responses, ciphertext records, attachments, and exports never do.
  return (
    url.pathname.startsWith('/assets/') ||
        url.pathname.startsWith('/wasm/') ||
    SHELL_URLS.includes(url.pathname)
  )
}

self.addEventListener('fetch', (event) => {
  const request = event.request
  const url = new URL(request.url)

  if (url.pathname.startsWith('/v1/')) {
    return
  }

  if (request.mode === 'navigate' && url.origin === self.location.origin) {
    event.respondWith(
      fetch(request).catch(async () => {
        const cachedShell = await caches.match('/')
        return (
          cachedShell ||
          new Response('Sprout is unavailable offline until it has opened once.', {
            status: 503,
            headers: { 'Content-Type': 'text/plain; charset=utf-8' },
          })
        )
      }),
    )
    return
  }

  if (!isCacheableShellAsset(request, url)) {
    return
  }

  event.respondWith(
    caches.match(request).then(async (cached) => {
      if (cached) {
        return cached
      }

      const response = await fetch(request)
      if (response.ok && response.type === 'basic') {
        const cache = await caches.open(CACHE_VERSION)
        await cache.put(request, response.clone())
      }
      return response
    }),
  )
})
