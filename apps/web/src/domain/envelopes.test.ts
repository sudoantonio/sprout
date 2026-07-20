import { describe, expect, it } from 'vitest'
import { envelopeSigningMessage } from './envelopes'

const hex = (value: Uint8Array): string =>
  [...value].map((byte) => byte.toString(16).padStart(2, '0')).join('')

describe('resource key envelope signatures', () => {
  it('matches the server canonical integer and UUID byte layout', async () => {
    const encryptedKey = new Uint8Array([1, 2, 3, 4])
    const message = await envelopeSigningMessage(
      '11111111-1111-4111-8111-111111111111',
      {
        version: 1,
        resource_id: '22222222-2222-4222-8222-222222222222',
        epoch: 1,
        recipient_identity_id: '33333333-3333-4333-8333-333333333333',
        recipient_device_id: '44444444-4444-4444-8444-444444444444',
        recipient_device_key_version: 2,
        sender_device_key_version: 3,
      },
      encryptedKey,
    )
    const domain = new TextEncoder().encode(
      'sprout-resource-key-envelope-v2',
    )
    expect(message.byteLength).toBe(
      domain.byteLength + 16 + 2 + 16 + 4 + 16 + 16 + 4 + 4 + 32,
    )
    expect(hex(message.slice(0, domain.byteLength))).toBe(hex(domain))

    const fixedFields = message.slice(domain.byteLength, -32)
    expect(hex(fixedFields)).toBe(
      [
        '11111111111141118111111111111111',
        '0001',
        '22222222222242228222222222222222',
        '00000001',
        '33333333333343338333333333333333',
        '44444444444444448444444444444444',
        '00000002',
        '00000003',
      ].join(''),
    )
    expect(hex(message.slice(-32))).toBe(
      hex(
        new Uint8Array(
          await crypto.subtle.digest('SHA-256', encryptedKey),
        ),
      ),
    )
  })
})
