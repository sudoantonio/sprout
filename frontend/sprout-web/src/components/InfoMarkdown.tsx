import { useEffect, useState } from 'react'
import ReactMarkdown, { type Components } from 'react-markdown'
import rehypeHighlight from 'rehype-highlight'
import rehypeSlug from 'rehype-slug'
import remarkGfm from 'remark-gfm'
import { remarkQuotedHttpLinks } from '../domain/info-markdown'

const MarkdownImage: NonNullable<Components['img']> = ({ src, alt, title }) => {
  const [failed, setFailed] = useState(false)

  useEffect(() => setFailed(false), [src])

  const fallback = alt?.trim() || 'Immagine non disponibile'
  if (!src || failed) {
    return (
      <span
        className="tasklist-info-remote-image-fallback"
        role="img"
        aria-label={fallback}
        title={title}
      >
        {fallback}
      </span>
    )
  }

  return (
    <img
      src={src}
      alt={alt ?? ''}
      title={title}
      loading="lazy"
      decoding="async"
      referrerPolicy="no-referrer"
      onError={() => setFailed(true)}
    />
  )
}

const MarkdownLink: NonNullable<Components['a']> = ({
  href,
  title,
  children,
}) => {
  const external = /^https?:\/\//iu.test(href ?? '')
  return (
    <a
      href={href}
      title={title}
      {...(external
        ? { target: '_blank', rel: 'noreferrer noopener' }
        : undefined)}
    >
      {children}
    </a>
  )
}

const MarkdownTable: NonNullable<Components['table']> = ({ children }) => (
  <div className="tasklist-info-table-scroll">
    <table>{children}</table>
  </div>
)

const components: Components = {
  a: MarkdownLink,
  img: MarkdownImage,
  table: MarkdownTable,
}

export const InfoMarkdown = ({ children }: { children: string }) => {
  if (!children.trim()) {
    return (
      <p className="tasklist-info-placeholder">
        Inserisci testo, link e informazioni relative a questa task list.
      </p>
    )
  }

  return (
    <div className="tasklist-info-markdown">
      <ReactMarkdown
        remarkPlugins={[remarkGfm, remarkQuotedHttpLinks]}
        rehypePlugins={[rehypeSlug, [rehypeHighlight, { detect: true }]]}
        components={components}
      >
        {children}
      </ReactMarkdown>
    </div>
  )
}
