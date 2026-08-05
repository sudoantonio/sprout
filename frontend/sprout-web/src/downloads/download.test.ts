import { afterEach, describe, expect, it, vi } from 'vitest'
import { saveWithDownloadFallback } from './download'

afterEach(() => {
  vi.restoreAllMocks()
  delete (
    window as Window & {
      showSaveFilePicker?: unknown
    }
  ).showSaveFilePicker
})

describe('forced download fallback', () => {
  it('uses a standard browser download when the picker is unavailable', async () => {
    vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:opaque')
    vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {})
    const click = vi
      .spyOn(HTMLAnchorElement.prototype, 'click')
      .mockImplementation(() => {})

    await expect(
      saveWithDownloadFallback(
        new Blob([crypto.getRandomValues(new Uint8Array(16))]),
        `${crypto.randomUUID()}.archive`,
      ),
    ).resolves.toBe('standard-download')
    expect(click).toHaveBeenCalledTimes(1)
  })
})
