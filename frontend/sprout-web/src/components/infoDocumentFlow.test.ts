import { describe, expect, it } from 'vitest'
import type { InfoDocumentBlock } from '../domain/models'
import {
  ensureTextBlock,
  flowCollapseStates,
  moveFlowBlockToTextBoundary,
  removeFlowBlock,
  splitTextBlockWith,
  updateTextBlock,
} from './infoDocumentFlow'

const ids = (...values: string[]) => {
  let index = 0
  return () => values[index++]!
}

describe('infoDocumentFlow', () => {
  it('keeps attachments and pages inside a collapse across text blocks', () => {
    const blocks: InfoDocumentBlock[] = [
      { id: 't1', type: 'text', markdown: 'Prima\n:::collapse[closed] Capitolo\nNascosto' },
      { id: 'f1', type: 'file', blob_id: 'b1', file_name: 'file.pdf', content_type: 'application/pdf', plaintext_size: 1 },
      { id: 'p1', type: 'document', document_id: 'd1', title: 'Pagina' },
      { id: 't2', type: 'text', markdown: 'Ancora nascosto' },
      { id: 'i1', type: 'file', blob_id: 'b2', file_name: 'image.png', content_type: 'image/png', plaintext_size: 2 },
      { id: 't3', type: 'text', markdown: ':::collapse Capitolo successivo\nVisibile' },
      { id: 'f2', type: 'file', blob_id: 'b3', file_name: 'visible.pdf', content_type: 'application/pdf', plaintext_size: 3 },
    ]

    const states = flowCollapseStates(blocks)

    expect(states.get('t1')).toEqual({ inheritedCollapsed: false, hiddenByCollapse: false })
    expect(states.get('f1')?.hiddenByCollapse).toBe(true)
    expect(states.get('p1')?.hiddenByCollapse).toBe(true)
    expect(states.get('t2')?.hiddenByCollapse).toBe(true)
    expect(states.get('i1')?.hiddenByCollapse).toBe(true)
    expect(states.get('t3')).toEqual({ inheritedCollapsed: true, hiddenByCollapse: false })
    expect(states.get('f2')?.hiddenByCollapse).toBe(false)
  })

  it('preserves every block while editing an intermediate text block', () => {
    const blocks: InfoDocumentBlock[] = [
      { id: 't1', type: 'text', markdown: 'A' },
      { id: 'f1', type: 'file', blob_id: 'b1', file_name: 'one.pdf', content_type: 'application/pdf', plaintext_size: 1 },
      { id: 't2', type: 'text', markdown: 'B' },
      { id: 'p1', type: 'document', document_id: 'd1', title: 'Page' },
      { id: 't3', type: 'text', markdown: 'C' },
    ]
    const next = updateTextBlock(blocks, 't2', 'B2')
    expect(next.map((block) => block.id)).toEqual(['t1', 'f1', 't2', 'p1', 't3'])
    expect(next[2]).toMatchObject({ id: 't2', type: 'text', markdown: 'B2' })
  })

  it('splits the selected text block without regrouping attachments', () => {
    const blocks: InfoDocumentBlock[] = [{ id: 't1', type: 'text', markdown: 'A\nB' }]
    const file: InfoDocumentBlock = {
      id: 'f1', type: 'file', blob_id: 'b1', file_name: 'one.pdf', content_type: 'application/pdf', plaintext_size: 1,
    }
    const next = splitTextBlockWith(blocks, 't1', 'A', file, 'B', ids('t2'))
    expect(next).toEqual([
      { id: 't1', type: 'text', markdown: 'A' },
      file,
      { id: 't2', type: 'text', markdown: 'B' },
    ])
  })

  it('moves a media block into an intermediate text boundary exactly once', () => {
    const blocks: InfoDocumentBlock[] = [
      { id: 't1', type: 'text', markdown: 'A' },
      { id: 'f1', type: 'file', blob_id: 'b1', file_name: 'one.pdf', content_type: 'application/pdf', plaintext_size: 1 },
      { id: 't2', type: 'text', markdown: 'B\nC' },
      { id: 'p1', type: 'document', document_id: 'd1', title: 'Page' },
      { id: 't3', type: 'text', markdown: 'D' },
    ]
    const next = moveFlowBlockToTextBoundary(blocks, 'p1', 't2', 'B', 'C', ids('join', 't4'))
    expect(next.map((block) => block.id)).toEqual(['t1', 'f1', 't2', 'p1', 'join'])
    expect(next.filter((block) => block.id === 'p1')).toHaveLength(1)
    expect(next.at(-1)).toMatchObject({
      type: 'text',
      markdown: 'C\nD',
    })
  })

  it('joins the text blocks left adjacent at the source after a move', () => {
    const blocks: InfoDocumentBlock[] = [
      { id: 't1', type: 'text', markdown: 'Destinazione' },
      { id: 'f1', type: 'file', blob_id: 'b1', file_name: 'one.pdf', content_type: 'application/pdf', plaintext_size: 1 },
      { id: 't2', type: 'text', markdown: 'Prima del file' },
      { id: 'f2', type: 'file', blob_id: 'b2', file_name: 'two.png', content_type: 'image/png', plaintext_size: 2 },
      { id: 't3', type: 'text', markdown: 'Dopo il file' },
    ]

    const next = moveFlowBlockToTextBoundary(
      blocks,
      'f2',
      't1',
      'Destina',
      'zione',
      ids('destination-after'),
    )

    expect(next.map((block) => block.id)).toEqual([
      't1', 'f2', 'destination-after', 'f1', 't2',
    ])
    expect(next.at(-1)).toMatchObject({
      id: 't2',
      type: 'text',
      markdown: 'Prima del file\nDopo il file',
    })
    expect(next.some((block, index) => (
      block.type === 'text' && next[index + 1]?.type === 'text'
    ))).toBe(false)
  })

  it('joins adjacent text after removal and always keeps an editor', () => {
    const blocks: InfoDocumentBlock[] = [
      { id: 't1', type: 'text', markdown: 'A' },
      { id: 'f1', type: 'file', blob_id: 'b1', file_name: 'one.pdf', content_type: 'application/pdf', plaintext_size: 1 },
      { id: 't2', type: 'text', markdown: 'B' },
    ]
    expect(removeFlowBlock(blocks, 'f1').map((block) => (
      block.type === 'text' ? `${block.id}:${block.markdown}` : block.id
    ))).toEqual(['t1:A\nB'])
    expect(ensureTextBlock([], ids('t0'))).toEqual([
      { id: 't0', type: 'text', markdown: '' },
    ])
  })

  it('keeps edits made to multiple text blocks around mixed attachments', () => {
    const blocks: InfoDocumentBlock[] = [
      { id: 't1', type: 'text', markdown: 'Prima' },
      { id: 'f1', type: 'file', blob_id: 'b1', file_name: 'one.pdf', content_type: 'application/pdf', plaintext_size: 1 },
      { id: 't2', type: 'text', markdown: 'Centro' },
      { id: 'p1', type: 'document', document_id: 'd1', title: 'Pagina' },
      { id: 't3', type: 'text', markdown: 'Dopo' },
    ]

    const next = updateTextBlock(
      updateTextBlock(
        updateTextBlock(blocks, 't1', 'Prima modificata'),
        't2',
        'Centro modificato',
      ),
      't3',
      'Dopo modificato',
    )

    expect(next).toEqual([
      { id: 't1', type: 'text', markdown: 'Prima modificata' },
      blocks[1],
      { id: 't2', type: 'text', markdown: 'Centro modificato' },
      blocks[3],
      { id: 't3', type: 'text', markdown: 'Dopo modificato' },
    ])
    expect(blocks.filter((block) => block.type === 'text').map((block) => block.markdown))
      .toEqual(['Prima', 'Centro', 'Dopo'])
  })

  it('moves then removes mixed blocks without dropping unrelated content', () => {
    const blocks: InfoDocumentBlock[] = [
      { id: 't1', type: 'text', markdown: 'Uno' },
      { id: 'f1', type: 'file', blob_id: 'b1', file_name: 'one.pdf', content_type: 'application/pdf', plaintext_size: 1 },
      { id: 't2', type: 'text', markdown: 'Due\nTre' },
      { id: 'p1', type: 'document', document_id: 'd1', title: 'Pagina' },
      { id: 't3', type: 'text', markdown: 'Quattro' },
      { id: 'f2', type: 'file', blob_id: 'b2', file_name: 'two.png', content_type: 'image/png', plaintext_size: 2 },
      { id: 't4', type: 'text', markdown: 'Cinque' },
    ]

    const moved = moveFlowBlockToTextBoundary(
      blocks,
      'f2',
      't2',
      'Due',
      'Tre',
      ids('t2-after'),
    )
    expect(moved.map((block) => block.id)).toEqual([
      't1', 'f1', 't2', 'f2', 't2-after', 'p1', 't3',
    ])
    expect(moved.find((block) => block.id === 't3')).toMatchObject({
      type: 'text',
      markdown: 'Quattro\nCinque',
    })

    const removed = removeFlowBlock(moved, 'p1')
    expect(removed.map((block) => block.id)).toEqual([
      't1', 'f1', 't2', 'f2', 't2-after',
    ])
    expect(removed.find((block) => block.id === 't2-after')).toMatchObject({
      type: 'text',
      markdown: 'Tre\nQuattro\nCinque',
    })
    expect(removed.filter((block) => block.type === 'file').map((block) => block.file_name))
      .toEqual(['one.pdf', 'two.png'])
  })

  it('does not accumulate empty text editors after repeated moves', () => {
    const blocks: InfoDocumentBlock[] = [
      { id: 't1', type: 'text', markdown: 'Uno' },
      { id: 'f1', type: 'file', blob_id: 'b1', file_name: 'one.pdf', content_type: 'application/pdf', plaintext_size: 1 },
      { id: 't2', type: 'text', markdown: 'Due' },
    ]

    const first = moveFlowBlockToTextBoundary(
      blocks,
      'f1',
      't1',
      '',
      'Uno',
      ids('after-first'),
    )
    expect(first.map((block) => block.id)).toEqual(['t1', 'f1', 'after-first'])
    expect(first.filter((block) => block.type === 'text' && block.markdown === ''))
      .toHaveLength(1)

    const second = moveFlowBlockToTextBoundary(
      first,
      'f1',
      'after-first',
      'Uno',
      'Due',
      ids('after-second'),
    )
    expect(second.map((block) => block.id)).toEqual(['t1', 'f1', 'after-second'])
    expect(second.filter((block) => block.type === 'text' && block.markdown === ''))
      .toHaveLength(0)
    expect(second.filter((block) => block.id === 'f1')).toHaveLength(1)
    expect(second.filter((block) => block.type === 'text').map((block) => block.markdown))
      .toEqual(['Uno', 'Due'])
  })
})
