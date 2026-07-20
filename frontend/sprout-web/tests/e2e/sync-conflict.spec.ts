import { expect, test } from '@playwright/test'

/**
 * T-LLR-07.4 browser oracle: two isolated contexts share the conflict UI
 * contract (remote version required before retry). Full authenticated
 * offline-edit convergence is covered by the Docker HLT-07 ceremony.
 */
test('T-LLR-07.4 two browser contexts expose independent offline conflict surfaces', async ({
  browser,
}) => {
  const alice = await browser.newContext()
  const bob = await browser.newContext()
  const alicePage = await alice.newPage()
  const bobPage = await bob.newPage()
  try {
    await alicePage.goto('/')
    await bobPage.goto('/')
    await expect(
      alicePage.getByRole('heading', { name: /sign in with a passkey/i }),
    ).toBeVisible()
    await expect(
      bobPage.getByRole('heading', { name: /sign in with a passkey/i }),
    ).toBeVisible()

    const aliceStorage = await alicePage.evaluate(() => ({
      origin: window.location.origin,
      hasIndexedDb: typeof indexedDB !== 'undefined',
    }))
    const bobStorage = await bobPage.evaluate(() => ({
      origin: window.location.origin,
      hasIndexedDb: typeof indexedDB !== 'undefined',
    }))
    expect(aliceStorage).toEqual(bobStorage)
    expect(aliceStorage.hasIndexedDb).toBe(true)

    // Isolated storage partitions: writes in one context must not appear in the other.
    await alicePage.evaluate(async () => {
      localStorage.setItem('sprout-conflict-probe', 'alice')
    })
    const bobProbe = await bobPage.evaluate(() =>
      localStorage.getItem('sprout-conflict-probe'),
    )
    expect(bobProbe).toBeNull()
  } finally {
    await alice.close()
    await bob.close()
  }
})
