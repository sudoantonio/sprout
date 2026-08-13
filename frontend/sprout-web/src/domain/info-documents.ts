export type InfoTextSegment =
  | { type: 'text'; value: string }
  | { type: 'link'; value: string; href: string }

const HTTP_LINK = /"(https?:\/\/[^"\r\n]+)"|(https?:\/\/[^\s"]+)/giu

/**
 * Link recognition is deliberately client-side: the server only receives the
 * encrypted markdown document and never learns which strings are URLs.
 */
export function linkifyInfoText(value: string): InfoTextSegment[] {
  const segments: InfoTextSegment[] = []
  let cursor = 0
  for (const match of value.matchAll(HTTP_LINK)) {
    const index = match.index
    if (index > cursor) {
      segments.push({ type: 'text', value: value.slice(cursor, index) })
    }
    const href = match[1] ?? match[2]
    segments.push({ type: 'link', value: href, href })
    cursor = index + match[0].length
  }
  if (cursor < value.length) {
    segments.push({ type: 'text', value: value.slice(cursor) })
  }
  return segments.length > 0 ? segments : [{ type: 'text', value }]
}

export type InfoMarkdownLine =
  | { type: 'blank'; key: number }
  | { type: 'heading'; key: number; level: 1 | 2 | 3; value: string }
  | { type: 'list-item'; key: number; value: string }
  | { type: 'paragraph'; key: number; value: string }

export function parseInfoMarkdown(value: string): InfoMarkdownLine[] {
  return value.split('\n').map((line, key) => {
    if (!line.trim()) return { type: 'blank', key }
    const heading = /^(#{1,3})\s+(.+)$/.exec(line)
    if (heading) {
      return {
        type: 'heading',
        key,
        level: heading[1].length as 1 | 2 | 3,
        value: heading[2],
      }
    }
    const listItem = /^[-*]\s+(.+)$/.exec(line)
    if (listItem) return { type: 'list-item', key, value: listItem[1] }
    return { type: 'paragraph', key, value: line }
  })
}
