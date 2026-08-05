import { describe, expect, it } from 'vitest'
import type { RetentionArchiveDto } from '../api/contracts'
import { availableRetentionArchiveCount } from './retention'

const archive = (
  state: RetentionArchiveDto['state'],
  downloadedAt: string | null,
): RetentionArchiveDto =>
  ({
    id: crypto.randomUUID(),
    state,
    downloaded_at: downloadedAt,
  }) as RetentionArchiveDto

describe('retention archive next-login delivery', () => {
  it('notifies only successful archives not already downloaded', () => {
    expect(
      availableRetentionArchiveCount([
        archive('succeeded', null),
        archive('succeeded', new Date().toISOString()),
        archive('pending', null),
        archive('failed', null),
      ]),
    ).toBe(1)
  })
})
