import type { SVGProps } from 'react'

type IconProps = SVGProps<SVGSVGElement>

const iconProps = {
  viewBox: '0 0 24 24',
  width: 20,
  height: 20,
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 1.8,
  strokeLinecap: 'round',
  strokeLinejoin: 'round',
  'aria-hidden': true,
} as const

export const SproutIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <path d="M12 21v-9" />
    <path d="M12 13C7.7 13 5 10.4 5 6c4.3-.1 7 2.5 7 7Z" />
    <path d="M12 10c.3-3.7 2.7-5.8 6.5-6-.2 3.7-2.6 5.8-6.5 6Z" />
  </svg>
)

export const LockIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <rect x="5" y="10" width="14" height="11" rx="2" />
    <path d="M8 10V7a4 4 0 0 1 8 0v3" />
    <path d="M12 14v3" />
  </svg>
)

export const SearchIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <circle cx="11" cy="11" r="6.5" />
    <path d="m16 16 4 4" />
  </svg>
)

export const PlusIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <path d="M12 5v14M5 12h14" />
  </svg>
)

export const ChevronIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <path d="m9 6 6 6-6 6" />
  </svg>
)

export const RepeatIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <path d="m17 2 3 3-3 3" />
    <path d="M3 11V9a4 4 0 0 1 4-4h13" />
    <path d="m7 22-3-3 3-3" />
    <path d="M21 13v2a4 4 0 0 1-4 4H4" />
  </svg>
)

export const PaperclipIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <path d="m20.5 11.5-8.2 8.2a6 6 0 0 1-8.5-8.5l8.5-8.5a4 4 0 0 1 5.7 5.7l-8.6 8.5a2 2 0 1 1-2.8-2.8l7.8-7.8" />
  </svg>
)

export const DownloadIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <path d="M12 3v12" />
    <path d="m7 10 5 5 5-5" />
    <path d="M5 21h14" />
  </svg>
)

export const ShieldIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <path d="M12 22s8-3.5 8-10V5l-8-3-8 3v7c0 6.5 8 10 8 10Z" />
    <path d="m9 12 2 2 4-5" />
  </svg>
)

export const KeyIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <circle cx="8" cy="15" r="4" />
    <path d="m11 12 8-8M15 8l3 3M17 6l2 2" />
  </svg>
)

export const WifiOffIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <path d="m2 2 20 20" />
    <path d="M8.5 5.5A15.5 15.5 0 0 1 21 9" />
    <path d="M3 9a16 16 0 0 1 2.2-1.3" />
    <path d="M6.5 13a9 9 0 0 1 7.8-.9" />
    <path d="M9.5 17a4 4 0 0 1 5 0" />
    <path d="M12 21h.01" />
  </svg>
)
