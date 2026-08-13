import { describe, expect, it } from 'vitest'
import { linkifyInfoText, parseInfoMarkdown } from './info-documents'

describe('info document markdown', () => {
  it('recognizes unquoted and quoted HTTP links without server metadata', () => {
    expect(
      linkifyInfoText(
        'Apri https://sprout.test/docs oppure "http://example.test/a b" ora',
      ),
    ).toEqual([
      { type: 'text', value: 'Apri ' },
      {
        type: 'link',
        value: 'https://sprout.test/docs',
        href: 'https://sprout.test/docs',
      },
      { type: 'text', value: ' oppure ' },
      {
        type: 'link',
        value: 'http://example.test/a b',
        href: 'http://example.test/a b',
      },
      { type: 'text', value: ' ora' },
    ])
  })

  it('keeps lightweight markdown structure in the encrypted text block', () => {
    expect(parseInfoMarkdown('# Titolo\n- voce\nTesto')[0]).toEqual({
      type: 'heading',
      key: 0,
      level: 1,
      value: 'Titolo',
    })
  })
})
