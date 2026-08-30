import type { SVGProps } from 'react'
import { getTaskListGlyph } from './task-list-glyphs'

type TaskListGlyphIconProps = SVGProps<SVGSVGElement> & {
  glyphId: string
}

export const TaskListGlyphIcon = ({
  glyphId,
  ...props
}: TaskListGlyphIconProps) => {
  const glyph = getTaskListGlyph(glyphId)
  if (!glyph) return null

  return (
    <svg
      viewBox="0 0 24 24"
      width={20}
      height={20}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
      {...props}
    >
      {glyph.circles?.map((circle) => (
        <circle key={`${circle.cx}-${circle.cy}`} {...circle} />
      ))}
      {glyph.rects?.map((rect) => (
        <rect key={`${rect.x}-${rect.y}`} {...rect} />
      ))}
      {glyph.paths.map((path) => (
        <path key={path} d={path} />
      ))}
    </svg>
  )
}
