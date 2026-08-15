import type { Content, Link, Parent, Root, Text } from 'mdast'
import type { Plugin } from 'unified'

const QUOTED_HTTP_LINK = /"(https?:\/\/[^"\r\n]+)"/giu

const splitQuotedLinks = (node: Text): Content[] => {
  const children: Content[] = []
  let cursor = 0

  for (const match of node.value.matchAll(QUOTED_HTTP_LINK)) {
    const index = match.index
    if (index > cursor) {
      children.push({
        type: 'text',
        value: node.value.slice(cursor, index),
      })
    }
    const url = match[1]
    children.push({
      type: 'link',
      url,
      children: [{ type: 'text', value: url }],
    } satisfies Link)
    cursor = index + match[0].length
  }

  if (cursor < node.value.length) {
    children.push({ type: 'text', value: node.value.slice(cursor) })
  }
  return children.length > 0 ? children : [node]
}

const transformQuotedLinks = (parent: Parent) => {
  for (const child of parent.children) {
    if (
      'children' in child &&
      child.type !== 'link' &&
      child.type !== 'linkReference'
    ) {
      transformQuotedLinks(child)
    }
  }

  const combined: Content[] = []
  for (let index = 0; index < parent.children.length; index += 1) {
    const before = parent.children[index]
    const link = parent.children[index + 1]
    const after = parent.children[index + 2]
    if (
      before?.type === 'text' &&
      link?.type === 'link' &&
      /^https?:\/\//iu.test(link.url) &&
      after?.type === 'text'
    ) {
      const openingQuote = before.value.lastIndexOf('"')
      const closingQuote = after.value.indexOf('"')
      if (
        openingQuote >= 0 &&
        openingQuote === before.value.length - 1 &&
        closingQuote >= 0
      ) {
        const prefix = before.value.slice(0, openingQuote)
        const suffix = after.value.slice(closingQuote + 1)
        const url = `${link.url}${after.value.slice(0, closingQuote)}`
        if (prefix) combined.push({ type: 'text', value: prefix })
        combined.push({
          ...link,
          url,
          children: [{ type: 'text', value: url }],
        })
        if (suffix) combined.push({ type: 'text', value: suffix })
        index += 2
        continue
      }
    }
    combined.push(before)
  }

  parent.children = combined.flatMap((child) =>
    child.type === 'text' ? splitQuotedLinks(child) : [child],
  )
}

/**
 * Preserves Sprout's original quoted-URL convention while GFM handles normal
 * URL literals. Text inside code and existing links is never rewritten.
 */
export const remarkQuotedHttpLinks: Plugin<[], Root> = () => (tree) => {
  transformQuotedLinks(tree)
}
