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

export const ChevronUpIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <path d="m6 15 6-6 6 6" />
  </svg>
)

export const ChevronDownIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <path d="m6 9 6 6 6-6" />
  </svg>
)

export const CalendarIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <rect x="4" y="5" width="16" height="15" rx="2" />
    <path d="M8 3v4M16 3v4" />
    <path d="M4 10h16" />
    <circle cx="9" cy="15" r="1" fill="currentColor" stroke="none" />
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

export const SidebarCollapseIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <rect x="4" y="5" width="16" height="14" rx="2" />
    <path d="M9 5v14" />
  </svg>
)

export const SidebarExpandIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <rect x="4" y="5" width="16" height="14" rx="2" />
    <path d="M15 5v14" />
  </svg>
)

export const LayoutGridIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <rect x="3" y="3" width="7" height="7" rx="1" />
    <rect x="14" y="3" width="7" height="7" rx="1" />
    <rect x="3" y="14" width="7" height="7" rx="1" />
    <rect x="14" y="14" width="7" height="7" rx="1" />
  </svg>
)

export const UsersIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2" />
    <circle cx="9" cy="7" r="4" />
    <path d="M22 21v-2a4 4 0 0 0-3-3.87" />
    <path d="M16 3.13a4 4 0 0 1 0 7.75" />
  </svg>
)

export const SlidersIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <path d="M4 21v-7" />
    <path d="M4 10V3" />
    <path d="M12 21v-9" />
    <path d="M12 8V3" />
    <path d="M20 21v-5" />
    <path d="M20 12V3" />
    <circle cx="4" cy="14" r="2" />
    <circle cx="12" cy="12" r="2" />
    <circle cx="20" cy="17" r="2" />
  </svg>
)

export const ClipboardListIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <rect x="8" y="2" width="8" height="4" rx="1" />
    <path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2" />
    <path d="M9 12h6" />
    <path d="M9 16h6" />
    <path d="M9 8h6" />
  </svg>
)

export const RefreshCwIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" />
    <path d="M21 3v5h-5" />
    <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" />
    <path d="M8 16H3v5" />
  </svg>
)

export const ClockIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <circle cx="12" cy="12" r="9" />
    <path d="M12 7v5l3 3" />
  </svg>
)

export const AlertTriangleIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z" />
    <path d="M12 9v4" />
    <path d="M12 17h.01" />
  </svg>
)

export const LogOutIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" />
    <path d="m16 17 5-5-5-5" />
    <path d="M21 12H9" />
  </svg>
)

export const FolderIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.93a2 2 0 0 1-1.66-.9l-.82-1.2A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />
  </svg>
)

export const PaletteIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <circle cx="13.5" cy="6.5" r="0.5" fill="currentColor" stroke="none" />
    <circle cx="17.5" cy="10.5" r="0.5" fill="currentColor" stroke="none" />
    <circle cx="8.5" cy="7.5" r="0.5" fill="currentColor" stroke="none" />
    <circle cx="6.5" cy="12" r="0.5" fill="currentColor" stroke="none" />
    <path d="M12 2a10 10 0 0 0-1 19.9V22a1 1 0 0 0 1 1h1a1 1 0 0 0 1-1v-1.1A10 10 0 0 0 12 2Z" />
  </svg>
)

export const CheckIcon = (props: IconProps) => (
  <svg {...iconProps} {...props}>
    <path d="m5 12 4 4 10-10" />
  </svg>
)
