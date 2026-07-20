interface SaveFilePickerOptions {
  suggestedName?: string
  types?: Array<{
    description?: string
    accept: Record<string, string[]>
  }>
}

interface SaveFileWindow extends Window {
  showSaveFilePicker?: (
    options?: SaveFilePickerOptions,
  ) => Promise<FileSystemFileHandle>
}

export const safeDownloadFileName = (fileName: string): string => {
  const sanitized = Array.from(fileName, (character) => {
    const codePoint = character.codePointAt(0) ?? 0
    return codePoint <= 0x1f || /[\\/:*?"<>|]/.test(character)
      ? '-'
      : character
  })
    .join('')
    .trim()
    .replace(/\.{2,}/g, '-')
    .replace(/^[-.]+/, '')
  return sanitized || 'sprout-download.bin'
}

export const asSafeDownloadBlob = (blob: Blob): Blob =>
  new Blob([blob], { type: 'application/octet-stream' })

export const standardDownload = (blob: Blob, fileName: string): void => {
  const objectUrl = URL.createObjectURL(asSafeDownloadBlob(blob))
  const anchor = document.createElement('a')
  anchor.href = objectUrl
  anchor.download = safeDownloadFileName(fileName)
  anchor.rel = 'noopener'
  anchor.click()
  window.setTimeout(() => URL.revokeObjectURL(objectUrl), 1_000)
}

export const saveWithDownloadFallback = async (
  blob: Blob,
  fileName: string,
): Promise<'file-picker' | 'standard-download'> => {
  const picker = (window as SaveFileWindow).showSaveFilePicker
  if (picker) {
    try {
      const handle = await picker({
        suggestedName: safeDownloadFileName(fileName),
        types: [
          {
            description: 'Sprout encrypted data',
            accept: {
              'application/octet-stream': ['.sprout', '.enc'],
            },
          },
        ],
      })
      const writable = await handle.createWritable()
      await writable.write(asSafeDownloadBlob(blob))
      await writable.close()
      return 'file-picker'
    } catch (error) {
      if (error instanceof DOMException && error.name === 'AbortError') {
        throw error
      }
    }
  }

  standardDownload(blob, fileName)
  return 'standard-download'
}
