import { flattenEmojiData, type Emoji } from 'emojibase'

export type TaskListEmojiEntry = {
  emoji: string
  label: string
  tags: string[]
  group: number
  hexcode: string
}

type EmojiMessages = {
  groups: Array<{ order: number; message: string }>
}

const COMPONENT_GROUP = 2

let emojiCache: TaskListEmojiEntry[] | null = null
let groupLabelCache: Record<number, string> | null = null

function isPickableEmoji(entry: Emoji): entry is Emoji & { emoji: string; group: number } {
  return (
    Boolean(entry.emoji) &&
    entry.group !== undefined &&
    entry.group !== COMPONENT_GROUP &&
    !entry.label.startsWith('regional indicator')
  )
}

function titleCaseGroupLabel(message: string): string {
  if (!message) return 'Altro'
  return message.charAt(0).toUpperCase() + message.slice(1)
}

async function loadGroupLabels(): Promise<Record<number, string>> {
  if (groupLabelCache) return groupLabelCache
  const module = await import('emojibase-data/it/messages.json')
  const messages = module.default as EmojiMessages
  groupLabelCache = Object.fromEntries(
    messages.groups.map((group) => [group.order, titleCaseGroupLabel(group.message)]),
  )
  return groupLabelCache
}

export async function loadTaskListEmojis(): Promise<TaskListEmojiEntry[]> {
  if (emojiCache) return emojiCache
  const module = await import('emojibase-data/en/data.json')
  const data = module.default as Emoji[]
  emojiCache = flattenEmojiData(data)
    .filter(isPickableEmoji)
    .map((entry) => ({
      emoji: entry.emoji,
      label: entry.label,
      tags: entry.tags ?? [],
      group: entry.group,
      hexcode: entry.hexcode,
    }))
  return emojiCache
}

export async function emojiGroupLabel(group: number): Promise<string> {
  const labels = await loadGroupLabels()
  return labels[group] ?? 'Altro'
}

export function filterTaskListEmojis(
  emojis: TaskListEmojiEntry[],
  query: string,
): TaskListEmojiEntry[] {
  const normalized = query.trim().toLowerCase()
  if (!normalized) return emojis
  return emojis.filter((entry) => {
    if (entry.label.toLowerCase().includes(normalized)) return true
    if (entry.emoji.includes(normalized)) return true
    return entry.tags.some((tag) => tag.toLowerCase().includes(normalized))
  })
}

export type TaskListEmojiGroup = {
  group: number
  label: string
  entries: TaskListEmojiEntry[]
}

export async function groupTaskListEmojis(
  emojis: TaskListEmojiEntry[],
): Promise<TaskListEmojiGroup[]> {
  const labels = await loadGroupLabels()
  const groups = new Map<number, TaskListEmojiEntry[]>()
  for (const entry of emojis) {
    const current = groups.get(entry.group) ?? []
    current.push(entry)
    groups.set(entry.group, current)
  }
  return [...groups.entries()]
    .sort(([left], [right]) => left - right)
    .map(([group, entries]) => ({
      group,
      label: labels[group] ?? 'Altro',
      entries,
    }))
}
