import type { Uuid } from '../api/contracts'
import type { InfoDocumentBlock } from '../domain/models'

const textBlock = (id: Uuid, markdown: string): InfoDocumentBlock => ({
  id,
  type: 'text',
  markdown,
})

export const ensureTextBlock = (
  blocks: InfoDocumentBlock[],
  createId: () => Uuid = () => crypto.randomUUID(),
): InfoDocumentBlock[] => (
  blocks.some((block) => block.type === 'text')
    ? blocks
    : [textBlock(createId(), ''), ...blocks]
)

export const updateTextBlock = (
  blocks: InfoDocumentBlock[],
  blockId: Uuid,
  markdown: string,
): InfoDocumentBlock[] => blocks.map((block) => (
  block.id === blockId && block.type === 'text'
    ? { ...block, markdown }
    : block
))

export const splitTextBlockWith = (
  blocks: InfoDocumentBlock[],
  textBlockId: Uuid,
  beforeMarkdown: string,
  inserted: InfoDocumentBlock,
  afterMarkdown: string,
  createId: () => Uuid = () => crypto.randomUUID(),
): InfoDocumentBlock[] => {
  const index = blocks.findIndex((block) => (
    block.id === textBlockId && block.type === 'text'
  ))
  if (index < 0) return [...blocks, inserted, textBlock(createId(), '')]
  const source = blocks[index]
  if (source.type !== 'text') return blocks
  return [
    ...blocks.slice(0, index),
    { ...source, markdown: beforeMarkdown },
    inserted,
    textBlock(createId(), afterMarkdown),
    ...blocks.slice(index + 1),
  ]
}

export const mergeAdjacentTextBlocks = (
  blocks: InfoDocumentBlock[],
  createId: () => Uuid = () => crypto.randomUUID(),
): InfoDocumentBlock[] => {
  const merged: InfoDocumentBlock[] = []
  for (const block of blocks) {
    const previous = merged.at(-1)
    if (previous?.type === 'text' && block.type === 'text') {
      previous.markdown = [previous.markdown, block.markdown]
        .filter((value) => value.length > 0)
        .join('\n')
      continue
    }
    merged.push({ ...block })
  }
  return ensureTextBlock(merged, createId)
}

export const removeFlowBlock = (
  blocks: InfoDocumentBlock[],
  blockId: Uuid,
  createId: () => Uuid = () => crypto.randomUUID(),
): InfoDocumentBlock[] => mergeAdjacentTextBlocks(
  blocks.filter((block) => block.id !== blockId),
  createId,
)

export const moveFlowBlockToTextBoundary = (
  blocks: InfoDocumentBlock[],
  sourceId: Uuid,
  targetTextBlockId: Uuid,
  beforeMarkdown: string,
  afterMarkdown: string,
  createId: () => Uuid = () => crypto.randomUUID(),
): InfoDocumentBlock[] => {
  const source = blocks.find((block) => block.id === sourceId)
  if (!source || source.type === 'text' || source.id === targetTextBlockId) return blocks
  const withoutSource = blocks.filter((block) => block.id !== sourceId)
  return mergeAdjacentTextBlocks(
    splitTextBlockWith(
      withoutSource,
      targetTextBlockId,
      beforeMarkdown,
      source,
      afterMarkdown,
      createId,
    ),
    createId,
  )
}

const openCollapsePrefix = ':::collapse '
const closedCollapsePrefix = ':::collapse[closed] '

export interface FlowCollapseState {
  inheritedCollapsed: boolean
  hiddenByCollapse: boolean
}

/** Propagates collapse membership through text, file, image, and page blocks. */
export const flowCollapseStates = (
  blocks: InfoDocumentBlock[],
): Map<Uuid, FlowCollapseState> => {
  const states = new Map<Uuid, FlowCollapseState>()
  let collapsed = false

  for (const block of blocks) {
    const inheritedCollapsed = collapsed
    if (block.type !== 'text') {
      states.set(block.id, {
        inheritedCollapsed,
        hiddenByCollapse: inheritedCollapsed,
      })
      continue
    }

    let containsCollapseBoundary = false
    for (const line of block.markdown.split('\n')) {
      if (line.startsWith(closedCollapsePrefix)) {
        containsCollapseBoundary = true
        collapsed = true
      } else if (line.startsWith(openCollapsePrefix)) {
        containsCollapseBoundary = true
        collapsed = false
      }
    }
    states.set(block.id, {
      inheritedCollapsed,
      hiddenByCollapse: inheritedCollapsed && !containsCollapseBoundary,
    })
  }

  return states
}
