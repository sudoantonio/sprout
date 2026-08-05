import { expect, test } from '@playwright/test'

test('T-LLR-09.1 keeps queued ciphertext after persistence refusal and quota failure', async ({
  page,
}) => {
  await page.goto('/')

  const result = await page.evaluate(async () => {
    Object.defineProperty(navigator.storage, 'persisted', {
      configurable: true,
      value: async () => false,
    })
    Object.defineProperty(navigator.storage, 'persist', {
      configurable: true,
      value: async () => false,
    })
    const persistenceGranted =
      (await navigator.storage.persisted()) ||
      (await navigator.storage.persist())

    const database = await new Promise<IDBDatabase>((resolve, reject) => {
      const request = indexedDB.open('sprout-encrypted-workspace', 2)
      request.addEventListener('success', () => resolve(request.result))
      request.addEventListener('error', () => reject(request.error))
    })
    const queuedAt = new Date().toISOString()
    await new Promise<void>((resolve, reject) => {
      const transaction = database.transaction(
        'encrypted-sync-queue',
        'readwrite',
      )
      transaction.objectStore('encrypted-sync-queue').put({
        id: crypto.randomUUID(),
        queuedAt,
        attempts: 0,
        request: {
          project_id: crypto.randomUUID(),
          encrypted_payload_b64: 'Y2lwaGVydGV4dA==',
        },
      })
      transaction.addEventListener('complete', () => resolve())
      transaction.addEventListener('error', () => reject(transaction.error))
    })

    const originalPut = IDBObjectStore.prototype.put
    Object.defineProperty(IDBObjectStore.prototype, 'put', {
      configurable: true,
      value(this: IDBObjectStore, value: unknown, key?: IDBValidKey) {
        if (this.name === 'encrypted-records') {
          throw new DOMException('Injected storage exhaustion', 'QuotaExceededError')
        }
        return originalPut.call(this, value, key)
      },
    })
    let quotaError = ''
    try {
      const transaction = database.transaction('encrypted-records', 'readwrite')
      transaction.objectStore('encrypted-records').put({
        id: crypto.randomUUID(),
      })
    } catch (error) {
      quotaError = error instanceof DOMException ? error.name : 'unknown'
    } finally {
      Object.defineProperty(IDBObjectStore.prototype, 'put', {
        configurable: true,
        value: originalPut,
      })
    }

    const queueCount = await new Promise<number>((resolve, reject) => {
      const transaction = database.transaction(
        'encrypted-sync-queue',
        'readonly',
      )
      const request = transaction.objectStore('encrypted-sync-queue').count()
      request.addEventListener('success', () => resolve(request.result))
      request.addEventListener('error', () => reject(request.error))
    })
    database.close()
    return { persistenceGranted, quotaError, queueCount }
  })

  expect(result).toEqual({
    persistenceGranted: false,
    quotaError: 'QuotaExceededError',
    queueCount: 1,
  })
})

test('T-LLR-09.3 excludes API secrets and plaintext from service-worker caches', async ({
  page,
}) => {
  const marker = `classified-${crypto.randomUUID()}`
  await page.route('**/v1/cache-probe**', (route) =>
    route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ marker, key_material: marker }),
    }),
  )
  await page.goto('/')
  await page.evaluate(async () => {
    const existing = await navigator.serviceWorker.getRegistration('/')
    if (existing) return existing
    const trustedTypes = Reflect.get(window, 'trustedTypes') as
      | {
          createPolicy(
            name: string,
            rules: { createScriptURL(value: string): string },
          ): { createScriptURL(value: string): string }
        }
      | undefined
    const scriptUrl =
      trustedTypes
        ?.createPolicy('sprout', {
          createScriptURL: (value) => value,
        })
        .createScriptURL('/sw.js') ?? '/sw.js'
    return navigator.serviceWorker.register(scriptUrl, {
      scope: '/',
      updateViaCache: 'none',
    })
  })
  await page.evaluate(() => navigator.serviceWorker.ready)
  await page.reload()
  await page.evaluate(async (classifiedMarker) => {
    const response = await fetch(
      `/v1/cache-probe?classified=${encodeURIComponent(classifiedMarker)}`,
    )
    await response.text()
  }, marker)

  const cacheSnapshot = await page.evaluate(async () => {
    const entries: Array<{ url: string; body: string }> = []
    for (const cacheName of await caches.keys()) {
      const cache = await caches.open(cacheName)
      for (const request of await cache.keys()) {
        const response = await cache.match(request)
        entries.push({
          url: request.url,
          body: response ? await response.text() : '',
        })
      }
    }
    return entries
  })

  expect(cacheSnapshot.length).toBeGreaterThan(0)
  expect(cacheSnapshot.some(({ url }) => new URL(url).pathname.startsWith('/v1/'))).toBe(
    false,
  )
  expect(JSON.stringify(cacheSnapshot)).not.toContain(marker)
})

test('T-LLR-09.5 blocks inline XSS and third-party scripts', async ({ page }) => {
  await page.goto('/')
  const result = await page.evaluate(async () => {
    Reflect.set(window, '__sproutXssExecuted', false)
    const image = document.createElement('img')
    let trustedTypesBlocked = false
    try {
      image.setAttribute('onerror', 'window.__sproutXssExecuted = true')
      image.src = '/definitely-missing-xss-fixture.png'
      document.body.append(image)
      await new Promise((resolve) => setTimeout(resolve, 100))
    } catch (error) {
      trustedTypesBlocked = error instanceof TypeError
    }
    return {
      executed: Reflect.get(window, '__sproutXssExecuted'),
      trustedTypesBlocked,
      trustedTypesAvailable: 'trustedTypes' in window,
      scriptSources: Array.from(document.scripts, (script) => script.src),
    }
  })

  expect(result.executed).toBe(false)
  expect(result.trustedTypesBlocked).toBe(result.trustedTypesAvailable)
  const externalSources = result.scriptSources.filter(Boolean)
  expect(externalSources.length).toBeGreaterThan(0)
  expect(
    externalSources.every(
      (source) => new URL(source).origin === 'http://127.0.0.1:4173',
    ),
  ).toBe(true)
})
