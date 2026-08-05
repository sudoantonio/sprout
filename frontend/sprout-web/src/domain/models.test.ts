import { describe, expect, it } from 'vitest'
import {
  defaultTaskListColumnColor,
  memberAvatarColor,
  normalizeTaskListColumnColor,
  resolveTaskListColumnTint,
  resolveTaskListDisplayColor,
  resolveTaskListIconColorFromStored,
} from './models'

describe('normalizeTaskListColumnColor', () => {
  it('keeps canonical color ids', () => {
    expect(normalizeTaskListColumnColor('column-peach')).toBe('column-peach')
  })

  it('remaps legacy aliases to nearest muted tone', () => {
    expect(normalizeTaskListColumnColor('column-green')).toBe('column-emerald')
    expect(normalizeTaskListColumnColor('column-pink')).toBe('column-rose')
    expect(normalizeTaskListColumnColor('lavender')).toBe('column-violet')
  })

  it('falls back when color is missing or unknown', () => {
    expect(normalizeTaskListColumnColor(undefined, 'column-sand')).toBe(
      'column-sand',
    )
    expect(normalizeTaskListColumnColor('unknown-tone')).toBe('column-blue')
  })
})

describe('defaultTaskListColumnColor', () => {
  it('picks a stable palette color from the list id', () => {
    const listId = 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee'
    expect(defaultTaskListColumnColor(listId)).toBe(
      defaultTaskListColumnColor(listId),
    )
    expect(defaultTaskListColumnColor(listId)).not.toBe(
      defaultTaskListColumnColor(`${listId}-other`),
    )
  })
})

describe('memberAvatarColor', () => {
  it('picks a stable palette color from the identity id', () => {
    const identityId = 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee'
    expect(memberAvatarColor(identityId)).toBe(memberAvatarColor(identityId))
    expect(memberAvatarColor(identityId)).not.toBe(
      memberAvatarColor(`${identityId}-other`),
    )
  })
})

describe('resolveTaskListDisplayColor', () => {
  it('uses the saved color when present', () => {
    expect(
      resolveTaskListDisplayColor({
        wire: { id: 'list-id' },
        document: { schema: 1, name: 'Lista', color: 'column-rose' },
      }),
    ).toBe('column-rose')
  })

  it('assigns a default palette color when none is saved', () => {
    expect(
      resolveTaskListDisplayColor({
        wire: { id: 'list-id' },
        document: { schema: 1, name: 'Lista' },
      }),
    ).toBe(defaultTaskListColumnColor('list-id'))
  })

  it('keeps a colored icon when the background is white', () => {
    expect(
      resolveTaskListDisplayColor({
        wire: { id: 'list-id' },
        document: { schema: 1, name: 'Lista', color: 'column-white' },
      }),
    ).toBe(defaultTaskListColumnColor('list-id'))
  })
})

describe('resolveTaskListColumnTint', () => {
  it('returns undefined for the default white background', () => {
    expect(resolveTaskListColumnTint(undefined)).toBeUndefined()
    expect(resolveTaskListColumnTint('column-white')).toBeUndefined()
  })

  it('keeps tint colors for colored backgrounds', () => {
    expect(resolveTaskListColumnTint('column-rose')).toBe('column-rose')
  })
})

describe('resolveTaskListIconColorFromStored', () => {
  it('maps legacy white aliases through normalize', () => {
    expect(resolveTaskListIconColorFromStored(undefined, 'list-id')).toBe(
      defaultTaskListColumnColor('list-id'),
    )
  })
})
