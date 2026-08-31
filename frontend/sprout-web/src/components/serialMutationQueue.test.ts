import { describe, expect, it } from 'vitest'
import { createSerialMutationQueue } from './serialMutationQueue'

describe('createSerialMutationQueue', () => {
  it('serializes autosave, upload, resize, rename and delete in call order', async () => {
    const queue = createSerialMutationQueue()
    const events: string[] = []
    const releases: Array<() => void> = []
    let running = 0
    let peakRunning = 0

    const enqueue = (name: string) => queue(async () => {
      events.push(`start:${name}`)
      running += 1
      peakRunning = Math.max(peakRunning, running)
      await new Promise<void>((resolve) => releases.push(resolve))
      running -= 1
      events.push(`end:${name}`)
      return name
    })

    const results = [
      enqueue('autosave'),
      enqueue('upload'),
      enqueue('resize'),
      enqueue('rename'),
      enqueue('delete'),
    ]

    await Promise.resolve()
    for (let index = 0; index < results.length; index += 1) {
      for (let attempt = 0; releases.length < index + 1 && attempt < 10; attempt += 1) {
        await Promise.resolve()
      }
      expect(releases).toHaveLength(index + 1)
      releases[index]!()
    }

    await expect(Promise.all(results)).resolves.toEqual([
      'autosave', 'upload', 'resize', 'rename', 'delete',
    ])
    expect(peakRunning).toBe(1)
    expect(events).toEqual([
      'start:autosave', 'end:autosave',
      'start:upload', 'end:upload',
      'start:resize', 'end:resize',
      'start:rename', 'end:rename',
      'start:delete', 'end:delete',
    ])
  })

  it('continues after an individual mutation fails', async () => {
    const queue = createSerialMutationQueue()
    const first = queue(async () => { throw new Error('conflict') })
    const second = queue(async () => 'saved')

    await expect(first).rejects.toThrow('conflict')
    await expect(second).resolves.toBe('saved')
  })
})
