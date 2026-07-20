import { afterEach, describe, expect, it, vi } from 'vitest'
import { requestPersistentStorage } from './pwa'

afterEach(() => {
  Reflect.deleteProperty(navigator, 'storage')
})

describe('persistent browser storage', () => {
  it('reuses an existing persistence grant', async () => {
    const persist = vi.fn()
    Object.defineProperty(navigator, 'storage', {
      configurable: true,
      value: {
        persisted: vi.fn().mockResolvedValue(true),
        persist,
      },
    })

    await expect(requestPersistentStorage()).resolves.toBe(true)
    expect(persist).not.toHaveBeenCalled()
  })

  it('requests persistence when the browser has not granted it', async () => {
    const persist = vi.fn().mockResolvedValue(true)
    Object.defineProperty(navigator, 'storage', {
      configurable: true,
      value: {
        persisted: vi.fn().mockResolvedValue(false),
        persist,
      },
    })

    await expect(requestPersistentStorage()).resolves.toBe(true)
    expect(persist).toHaveBeenCalledTimes(1)
  })

  it('reports a refused persistence request without hiding it', async () => {
    Object.defineProperty(navigator, 'storage', {
      configurable: true,
      value: {
        persisted: vi.fn().mockResolvedValue(false),
        persist: vi.fn().mockResolvedValue(false),
      },
    })

    await expect(requestPersistentStorage()).resolves.toBe(false)
  })

  it('reports unsupported persistence without throwing', async () => {
    Object.defineProperty(navigator, 'storage', {
      configurable: true,
      value: {},
    })

    await expect(requestPersistentStorage()).resolves.toBe(false)
  })
})
