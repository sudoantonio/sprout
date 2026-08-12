import { expect, test } from '@playwright/test'

test('HLT-09 upgrades a signed queue, works offline, and catches up', async ({
  browserName,
  context,
  page,
}) => {
  test.skip(
    browserName !== 'chromium',
    'The supported-browser matrix is covered separately by T-LLR-09.4',
  )

  await context.addInitScript(() => {
    Object.defineProperty(navigator.storage, 'persisted', {
      configurable: true,
      value: async () => false,
    })
    Object.defineProperty(navigator.storage, 'persist', {
      configurable: true,
      value: async () => false,
    })
  })

  // Establish the application origin without letting the v2 bundle open IDB.
  await page.route('**/assets/*.js', (route) => route.abort())
  await page.goto('/')

  const seed = await page.evaluate(async () => {
    const databaseName = 'sprout-encrypted-workspace'
    await new Promise<void>((resolve, reject) => {
      const request = indexedDB.deleteDatabase(databaseName)
      request.addEventListener('success', () => resolve())
      request.addEventListener('error', () => reject(request.error))
    })

    const projectId = crypto.randomUUID()
    const resourceId = crypto.randomUUID()
    const signedQueueItem = {
      id: crypto.randomUUID(),
      queuedAt: new Date().toISOString(),
      attempts: 0,
      request: {
        project_id: projectId,
        resource_node_id: resourceId,
        base_version: 0,
        aggregate_version: 1,
        actor_device_key_version: 1,
        device_sequence: 1,
        client_event_id: crypto.randomUUID(),
        event_kind: 'task.updated',
        mutation: 'upsert',
        key_epoch: 1,
        encrypted_payload_b64: 'Y2lwaGVydGV4dA==',
        previous_hash_b64: null,
        event_hash_b64: 'ZXZlbnQtaGFzaA==',
        classical_signature_b64: 'Y2xhc3NpY2FsLXNpZw==',
        post_quantum_signature_b64: 'cHEtc2ln',
        client_created_at: new Date().toISOString(),
        idempotency_key: crypto.randomUUID(),
      },
    }
    const database = await new Promise<IDBDatabase>((resolve, reject) => {
      const request = indexedDB.open(databaseName, 1)
      request.addEventListener('upgradeneeded', () => {
        request.result.createObjectStore('encrypted-sync-queue', {
          keyPath: 'id',
        })
        request.result.createObjectStore('legacy-local-projections', {
          keyPath: 'id',
        })
      })
      request.addEventListener('success', () => resolve(request.result))
      request.addEventListener('error', () => reject(request.error))
    })
    await new Promise<void>((resolve, reject) => {
      const transaction = database.transaction(
        ['encrypted-sync-queue', 'legacy-local-projections'],
        'readwrite',
      )
      transaction
        .objectStore('encrypted-sync-queue')
        .put(signedQueueItem)
      transaction.objectStore('encrypted-sync-queue').put({
        id: crypto.randomUUID(),
        queuedAt: new Date().toISOString(),
        attempts: 0,
        request: {
          project_id: projectId,
          encrypted_payload_b64: 'dW5zaWduZWQ=',
        },
      })
      transaction.objectStore('legacy-local-projections').put({
        id: 'legacy-plaintext',
        title: 'must not survive',
      })
      transaction.addEventListener('complete', () => resolve())
      transaction.addEventListener('error', () => reject(transaction.error))
    })
    database.close()
    return {
      projectId,
      queueItemId: signedQueueItem.id,
    }
  })

  await page.unroute('**/assets/*.js')
  await page.reload()
  await expect(
    page.getByText('Workspace cifrato, solo sui tuoi device.'),
  ).toBeVisible()

  const upgraded = await page.evaluate(async () => {
    const database = await new Promise<IDBDatabase>((resolve, reject) => {
      const request = indexedDB.open('sprout-encrypted-workspace')
      request.addEventListener('success', () => resolve(request.result))
      request.addEventListener('error', () => reject(request.error))
    })
    const queue = await new Promise<unknown[]>((resolve, reject) => {
      const request = database
        .transaction('encrypted-sync-queue', 'readonly')
        .objectStore('encrypted-sync-queue')
        .getAll()
      request.addEventListener('success', () => resolve(request.result))
      request.addEventListener('error', () => reject(request.error))
    })
    const result = {
      version: database.version,
      stores: Array.from(database.objectStoreNames).sort(),
      queue,
      persistenceGranted:
        (await navigator.storage.persisted()) ||
        (await navigator.storage.persist()),
    }
    database.close()
    return result
  })

  expect(upgraded.version).toBe(2)
  expect(upgraded.stores).toEqual([
    'encrypted-conflicts',
    'encrypted-key-vault',
    'encrypted-records',
    'encrypted-sync-queue',
    'sync-metadata',
    'sync-tombstones',
  ])
  expect(upgraded.queue).toHaveLength(1)
  expect(
    (upgraded.queue[0] as { id: string }).id,
  ).toBe(seed.queueItemId)
  expect(upgraded.persistenceGranted).toBe(false)

  const manifest = await page.evaluate(() =>
    fetch('/manifest.webmanifest').then((response) => response.json()),
  )
  expect(manifest).toMatchObject({ display: 'standalone', start_url: '/' })
  await page.evaluate(() => navigator.serviceWorker.ready)

  // Reload once under SW control so generated assets enter the shell cache.
  await page.reload()
  await expect(
    page.getByText('Workspace cifrato, solo sui tuoi device.'),
  ).toBeVisible()
  await expect
    .poll(() =>
      page.evaluate(async () => {
        for (const name of await caches.keys()) {
          const cache = await caches.open(name)
          if (
            (await cache.keys()).some(
              (request) => new URL(request.url).pathname.startsWith('/assets/'),
            )
          ) {
            return true
          }
        }
        return false
      }),
    )
    .toBe(true)

  await context.setOffline(true)
  await page.reload({ waitUntil: 'domcontentloaded' })
  await expect(
    page.getByText('Workspace cifrato, solo sui tuoi device.'),
  ).toBeVisible()

  let pushed = 0
  await context.setOffline(false)
  await page.route('**/v1/sync/push', async (route) => {
    const request = route.request().postDataJSON() as {
      resource_node_id: string
      aggregate_version: number
    }
    pushed += 1
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        projection: {
          resource_node_id: request.resource_node_id,
          aggregate_version: request.aggregate_version,
        },
      }),
    })
  })

  const pendingAfterCatchUp = await page.evaluate(async (projectId) => {
    const database = await new Promise<IDBDatabase>((resolve, reject) => {
      const request = indexedDB.open('sprout-encrypted-workspace')
      request.addEventListener('success', () => resolve(request.result))
      request.addEventListener('error', () => reject(request.error))
    })
    const queue = await new Promise<
      Array<{
        id: string
        request: { project_id: string }
      }>
    >((resolve, reject) => {
      const request = database
        .transaction('encrypted-sync-queue', 'readonly')
        .objectStore('encrypted-sync-queue')
        .getAll()
      request.addEventListener('success', () => resolve(request.result))
      request.addEventListener('error', () => reject(request.error))
    })
    for (const item of queue) {
      if (item.request.project_id !== projectId) continue
      const response = await fetch('/v1/sync/push', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(item.request),
      })
      if (!response.ok) continue
      await new Promise<void>((resolve, reject) => {
        const transaction = database.transaction(
          'encrypted-sync-queue',
          'readwrite',
        )
        transaction.objectStore('encrypted-sync-queue').delete(item.id)
        transaction.addEventListener('complete', () => resolve())
        transaction.addEventListener('error', () =>
          reject(transaction.error),
        )
      })
    }
    const countTransaction = database.transaction(
      'encrypted-sync-queue',
      'readonly',
    )
    const count = await new Promise<number>((resolve, reject) => {
      const request = countTransaction
        .objectStore('encrypted-sync-queue')
        .count()
      request.addEventListener('success', () => resolve(request.result))
      request.addEventListener('error', () => reject(request.error))
    })
    database.close()
    return count
  }, seed.projectId)

  expect(pushed).toBe(1)
  expect(pendingAfterCatchUp).toBe(0)
})
