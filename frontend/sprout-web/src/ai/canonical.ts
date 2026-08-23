const encoder = new TextEncoder()

const escapeString = (value: string): string => JSON.stringify(value)

const compareUtf16 = (left: string, right: string): number => {
  const leftUnits = Array.from(left).flatMap((character) => {
    const units: number[] = []
    for (let index = 0; index < character.length; index += 1) {
      units.push(character.charCodeAt(index))
    }
    return units
  })
  const rightUnits = Array.from(right).flatMap((character) => {
    const units: number[] = []
    for (let index = 0; index < character.length; index += 1) {
      units.push(character.charCodeAt(index))
    }
    return units
  })
  for (let index = 0; index < Math.min(leftUnits.length, rightUnits.length); index += 1) {
    const difference = leftUnits[index] - rightUnits[index]
    if (difference !== 0) return difference
  }
  return leftUnits.length - rightUnits.length
}

const serialize = (value: unknown): string => {
  if (value === null) return 'null'
  if (typeof value === 'boolean') return value ? 'true' : 'false'
  if (typeof value === 'string') return escapeString(value)
  if (typeof value === 'number') {
    if (!Number.isSafeInteger(value)) {
      throw new Error('Canonical Sprout JSON permits only safe integers')
    }
    return String(value)
  }
  if (Array.isArray(value)) return `[${value.map(serialize).join(',')}]`
  if (typeof value === 'object') {
    const record = value as Record<string, unknown>
    const keys = Object.keys(record)
      .filter((key) => record[key] !== undefined)
      .sort(compareUtf16)
    return `{${keys
      .map((key) => `${escapeString(key)}:${serialize(record[key])}`)
      .join(',')}}`
  }
  throw new Error('Unsupported canonical Sprout JSON value')
}

export const canonicalGovernanceJson = (value: unknown): Uint8Array =>
  encoder.encode(serialize(value))

export const sha256Hex = async (value: Uint8Array | string): Promise<string> => {
  const bytes = typeof value === 'string' ? encoder.encode(value) : value
  const digest = await crypto.subtle.digest(
    'SHA-256',
    bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer,
  )
  return Array.from(new Uint8Array(digest), (byte) =>
    byte.toString(16).padStart(2, '0'),
  ).join('')
}

export const utf8 = (value: string): Uint8Array => encoder.encode(value)

export const bytesToBase64 = (value: Uint8Array): string => {
  let binary = ''
  for (const byte of value) binary += String.fromCharCode(byte)
  return btoa(binary)
}
