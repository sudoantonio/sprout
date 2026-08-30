import { fireEvent, render, screen, within } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { InfoMarkdown } from './InfoMarkdown'

describe('InfoMarkdown', () => {
  it('renders remote images with title, alt text, lazy loading, and a failure fallback', () => {
    render(
      <InfoMarkdown>
        {'![Diagramma del flusso](https://images.example.test/flow.png "Architettura")'}
      </InfoMarkdown>,
    )

    const image = screen.getByRole('img', { name: 'Diagramma del flusso' })
    expect(image).toHaveAttribute(
      'src',
      'https://images.example.test/flow.png',
    )
    expect(image).toHaveAttribute('title', 'Architettura')
    expect(image).toHaveAttribute('loading', 'lazy')
    expect(image).toHaveAttribute('referrerpolicy', 'no-referrer')

    fireEvent.error(image)
    const fallback = screen.getByRole('img', { name: 'Diagramma del flusso' })
    expect(fallback).toHaveTextContent('Diagramma del flusso')
    expect(fallback).toHaveAttribute('title', 'Architettura')
  })

  it('supports GFM links, tables, strikethrough, references, and heading anchors', () => {
    render(
      <InfoMarkdown>{`# Piano di lancio

[Vai ai dettagli](#dettagli)

https://sprout.test/docs

"http://example.test/percorso con spazi"

~~obsoleto~~

| Campo | Valore |
| --- | --- |
| **Stato** | [documentazione][docs] |

## Dettagli

[docs]: https://example.test/reference "Riferimento"`}</InfoMarkdown>,
    )

    expect(screen.getByRole('heading', { name: 'Piano di lancio' })).toHaveAttribute(
      'id',
      'piano-di-lancio',
    )
    expect(screen.getByRole('heading', { name: 'Dettagli' })).toHaveAttribute(
      'id',
      'dettagli',
    )
    expect(screen.getByRole('link', { name: 'Vai ai dettagli' })).toHaveAttribute(
      'href',
      '#dettagli',
    )
    expect(screen.getByRole('link', { name: 'https://sprout.test/docs' })).toHaveAttribute(
      'href',
      'https://sprout.test/docs',
    )
    expect(
      screen.getByRole('link', {
        name: 'http://example.test/percorso con spazi',
      }),
    ).toHaveAttribute('href', 'http://example.test/percorso%20con%20spazi')
    expect(screen.getByText('obsoleto').tagName).toBe('DEL')

    const table = screen.getByRole('table')
    expect(within(table).getByText('Stato').tagName).toBe('STRONG')
    expect(within(table).getByRole('link', { name: 'documentazione' })).toHaveAttribute(
      'title',
      'Riferimento',
    )
  })

  it('preserves hard breaks, paragraph newlines, and Markdown escaping', () => {
    const { container } = render(
      <InfoMarkdown>{`prima riga${'  '}
seconda riga

nuovo paragrafo

\\*non corsivo\\*`}</InfoMarkdown>,
    )

    const paragraphs = container.querySelectorAll('p')
    expect(paragraphs).toHaveLength(3)
    expect(paragraphs[0].querySelector('br')).not.toBeNull()
    expect(paragraphs[0]).toHaveTextContent('prima riga seconda riga')
    expect(screen.getByText('*non corsivo*').tagName).not.toBe('EM')
  })

  it('renders nested lists and preserves a non-one ordered-list start', () => {
    const { container } = render(
      <InfoMarkdown>{`- primo
  - annidato
    1. uno
    2. due

4. quarto
5. quinto`}</InfoMarkdown>,
    )

    const outerList = container.querySelector('ul')
    expect(outerList).not.toBeNull()
    expect(outerList?.querySelector('ul ol')).not.toBeNull()
    const orderedLists = container.querySelectorAll('ol')
    expect(orderedLists).toHaveLength(2)
    expect(orderedLists[1]).toHaveAttribute('start', '4')
    expect(orderedLists[1].querySelectorAll(':scope > li')).toHaveLength(2)
  })

  it('renders nested blockquotes with inline Markdown', () => {
    const { container } = render(
      <InfoMarkdown>{`> **Nota importante**
>
> > Dettaglio con [link](https://example.test)`}</InfoMarkdown>,
    )

    const quote = container.querySelector('blockquote')
    expect(quote).not.toBeNull()
    expect(quote?.querySelector('strong')).toHaveTextContent('Nota importante')
    expect(quote?.querySelector('blockquote')).not.toBeNull()
    expect(within(quote as HTMLElement).getByRole('link', { name: 'link' })).toHaveAttribute(
      'href',
      'https://example.test',
    )
  })

  it('handles backticks in inline code and highlights a complete JavaScript block', () => {
    const { container } = render(
      <InfoMarkdown>{`Usa \`\` \`codice\` \`\` qui.

\`\`\`javascript
function greet(name) {
  const message = \`Ciao, \${name}!\`
  return message
}

console.log(greet('Sprout'))
\`\`\``}</InfoMarkdown>,
    )

    const inline = container.querySelector('p code')
    expect(inline).toHaveTextContent('`codice`')

    const block = container.querySelector('pre code')
    expect(block).toHaveClass('hljs', 'language-javascript')
    expect(block).toHaveTextContent('function greet(name)')
    expect(block).toHaveTextContent('console.log')
    expect(block).toHaveTextContent("greet('Sprout')")
  })
})
