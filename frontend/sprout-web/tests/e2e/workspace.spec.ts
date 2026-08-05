import { expect, test } from '@playwright/test'

test('T-LLR-09.4 runs the real PWA in every configured browser', async ({
  page,
}) => {
  await page.goto('/')
  await expect(
    page.getByRole('heading', {
      name: /your work stays readable only on authorized devices/i,
    }),
  ).toBeVisible()
  await expect(
    page.getByRole('heading', { name: /sign in with a passkey/i }),
  ).toBeVisible()
  await expect(page.getByText(/home|studio launch|demo ciphertext/i)).toHaveCount(
    0,
  )
})

test('T-LLR-09.5 ships strict CSP and Trusted Types policy', async ({
  page,
}) => {
  await page.goto('/')
  const policy = await page
    .locator('meta[http-equiv="Content-Security-Policy"]')
    .getAttribute('content')
  expect(policy).toContain("object-src 'none'")
  expect(policy).toContain("base-uri 'none'")
  expect(policy).toContain("require-trusted-types-for 'script'")
  expect(policy?.split(/\s+/)).not.toContain("'unsafe-inline'")
  expect(policy?.split(/\s+/)).not.toContain("'unsafe-eval'")

  const storage = await page.evaluate(async () => ({
    persistFunction: typeof navigator.storage?.persist === 'function',
    persisted:
      typeof navigator.storage?.persisted === 'function'
        ? await navigator.storage.persisted()
        : null,
    opfsFunction:
      typeof (
        navigator.storage as StorageManager & {
          getDirectory?: unknown
        }
      )?.getDirectory === 'function',
  }))
  expect(storage.persistFunction).toBe(true)
  expect(typeof storage.persisted).toBe('boolean')
  expect(typeof storage.opfsFunction).toBe('boolean')

  const manifest = await page.evaluate(() =>
    fetch('/manifest.webmanifest').then((response) => response.json()),
  )
  expect(manifest).toMatchObject({
    short_name: 'Sprout',
    display: 'standalone',
    start_url: '/',
  })
  expect(manifest.icons.length).toBeGreaterThan(0)

  const serviceWorker = await page.evaluate(() =>
    fetch('/sw.js').then((response) => response.text()),
  )
  expect(serviceWorker).toContain("url.pathname.startsWith('/v1/')")
  expect(serviceWorker).toContain('isCacheableShellAsset(request, url)')
})

test('disables network ceremonies after the browser goes offline', async ({
  context,
  page,
}) => {
  await page.goto('/')
  await context.setOffline(true)
  await page.evaluate(() => window.dispatchEvent(new Event('offline')))
  await expect(
    page.getByText(/account ceremonies require a network connection/i),
  ).toBeVisible()
  await expect(page.getByRole('button', { name: 'Use passkey' })).toBeDisabled()
})
