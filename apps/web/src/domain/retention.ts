import type { RetentionArchiveDto } from '../api/contracts'

export const availableRetentionArchiveCount = (
  archives: RetentionArchiveDto[],
): number =>
  archives.filter(
    (archive) =>
      archive.state === 'succeeded' && archive.downloaded_at === null,
  ).length
