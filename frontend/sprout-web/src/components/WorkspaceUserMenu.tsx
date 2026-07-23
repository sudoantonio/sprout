import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ComponentType,
  type CSSProperties,
  type FormEvent,
  type SVGProps,
} from 'react'
import { createPortal } from 'react-dom'
import type { AppearanceOption } from '../theme'
import type { ProjectItem } from '../store/app-store'
import type { AppScreen } from '../store/app-store'
import {
  AlertTriangleIcon,
  CheckIcon,
  ChevronDownIcon,
  ChevronUpIcon,
  ClipboardListIcon,
  ClockIcon,
  FolderIcon,
  LayoutGridIcon,
  LockIcon,
  LogOutIcon,
  PaletteIcon,
  PaperclipIcon,
  PlusIcon,
  RefreshCwIcon,
  SlidersIcon,
  UsersIcon,
} from './icons'

type MenuIcon = ComponentType<SVGProps<SVGSVGElement>>

const menuItems: Array<{ id: AppScreen; label: string; icon: MenuIcon }> = [
  { id: 'tasks', label: 'Board', icon: LayoutGridIcon },
  { id: 'people', label: 'Persone', icon: UsersIcon },
  { id: 'presets', label: 'Preset', icon: SlidersIcon },
  { id: 'questionnaires', label: 'Questionari', icon: ClipboardListIcon },
  { id: 'attachments', label: 'Allegati', icon: PaperclipIcon },
  { id: 'recovery', label: 'Recovery', icon: RefreshCwIcon },
  { id: 'retention', label: 'Retention', icon: ClockIcon },
  { id: 'conflicts', label: 'Conflitti', icon: AlertTriangleIcon },
  { id: 'security', label: 'Sicurezza', icon: LockIcon },
]

const USER_ID_LABEL = /^User\s+[a-f0-9]{6,}$/i

const displayLabelFor = (label: string): string => {
  const trimmed = label.trim()
  if (!trimmed) return 'Utente'
  if (USER_ID_LABEL.test(trimmed)) return 'Utente'
  return trimmed
}

const initialsFor = (label: string): string => {
  const display = displayLabelFor(label)
  const parts = display.split(/\s+/).filter(Boolean)
  if (parts.length === 0) return '?'
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase()
  return `${parts[0][0] ?? ''}${parts[1][0] ?? ''}`.toUpperCase()
}

export interface WorkspaceUserMenuProps {
  userLabel: string
  projects: ProjectItem[]
  selectedProjectId?: string
  currentScreen: AppScreen
  conflictCount: number
  projectName: string
  onProjectNameChange(name: string): void
  onSelectProject(projectId: string): void
  onCreateProject(event: FormEvent): void
  onNavigate(screen: AppScreen): void
  onLogout(): void
  appearance: AppearanceOption
  onAppearanceChange(appearance: AppearanceOption): void
  variant?: 'sidebar' | 'compact' | 'overview'
}

type WorkspaceUserPanelProps = Omit<WorkspaceUserMenuProps, 'variant' | 'userLabel'> & {
  onClose?(): void
}

const appearanceOptions: Array<{ value: AppearanceOption; label: string }> = [
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
  { value: 'system', label: 'System' },
  { value: 'tactical-light', label: 'Tactical Light' },
  { value: 'tactical-shadow', label: 'Tactical Shadow' },
]

const appearanceLabelFor = (value: AppearanceOption): string =>
  appearanceOptions.find((option) => option.value === value)?.label ?? 'System'

const WorkspaceUserPanel = ({
  projects,
  selectedProjectId,
  currentScreen,
  conflictCount,
  projectName,
  onProjectNameChange,
  onSelectProject,
  onCreateProject,
  onNavigate,
  onLogout,
  appearance,
  onAppearanceChange,
  onClose,
}: WorkspaceUserPanelProps) => {
  const [appearanceOpen, setAppearanceOpen] = useState(false)

  const navigate = (screen: AppScreen) => {
    onNavigate(screen)
    onClose?.()
  }

  const selectAppearance = (option: AppearanceOption) => {
    onAppearanceChange(option)
    setAppearanceOpen(false)
  }

  return (
    <>
      <div className="workspace-user-popover-section">
        <p className="workspace-user-popover-heading">Progetto</p>
        <label className="workspace-user-field workspace-user-field--with-icon">
          <FolderIcon className="workspace-user-field-icon" aria-hidden />
          <span className="sr-only">Progetto attivo</span>
          <select
            value={selectedProjectId ?? ''}
            onChange={(event) => onSelectProject(event.target.value)}
          >
            <option value="" disabled>
              Seleziona progetto
            </option>
            {projects.map((project) => (
              <option key={project.wire.id} value={project.wire.id}>
                {project.document?.name ??
                  `Locked ${project.wire.id.slice(0, 8)}`}
              </option>
            ))}
          </select>
        </label>
        <details className="workspace-user-create">
          <summary>
            <PlusIcon className="workspace-user-summary-icon" aria-hidden />
            Nuovo progetto cifrato
          </summary>
          <form
            onSubmit={(event) => {
              void onCreateProject(event)
              onClose?.()
            }}
          >
            <input
              required
              placeholder="Nome privato"
              value={projectName}
              onChange={(event) => onProjectNameChange(event.target.value)}
            />
            <button type="submit" className="secondary-button">
              Crea
            </button>
          </form>
        </details>
      </div>

      <div className="workspace-user-popover-section">
        <p className="workspace-user-popover-heading">Workspace</p>
        <ul className="workspace-user-nav" role="none">
          {menuItems.map((item) => {
            const Icon = item.icon
            return (
              <li key={item.id} role="none">
                <button
                  type="button"
                  role="menuitem"
                  className={
                    currentScreen === item.id
                      ? 'workspace-user-nav-item active'
                      : 'workspace-user-nav-item'
                  }
                  onClick={() => navigate(item.id)}
                >
                  <span className="workspace-user-nav-item-label">
                    <Icon className="workspace-user-nav-icon" />
                    <span>{item.label}</span>
                  </span>
                  {item.id === 'conflicts' && conflictCount > 0 && (
                    <span className="count-badge">{conflictCount}</span>
                  )}
                </button>
              </li>
            )
          })}
        </ul>
      </div>

      <div className="workspace-user-popover-section">
        <ul className="workspace-user-nav" role="none">
          <li role="none" className="workspace-user-appearance">
            <button
              type="button"
              role="menuitem"
              className="workspace-user-nav-item workspace-user-appearance-trigger"
              aria-expanded={appearanceOpen}
              aria-haspopup="menu"
              aria-label={`Appearance, ${appearanceLabelFor(appearance)}`}
              onClick={() => setAppearanceOpen((value) => !value)}
            >
              <span className="workspace-user-nav-item-label">
                <PaletteIcon className="workspace-user-nav-icon" />
                <span>Appearance</span>
              </span>
              <span className="workspace-user-appearance-value">
                {appearanceLabelFor(appearance)}
              </span>
              <ChevronDownIcon
                className={
                  appearanceOpen
                    ? 'workspace-user-appearance-chevron open'
                    : 'workspace-user-appearance-chevron'
                }
              />
            </button>
            {appearanceOpen && (
              <ul
                className="workspace-user-appearance-menu"
                role="menu"
                aria-label="Interface appearance"
              >
                {appearanceOptions.map((option) => (
                  <li key={option.value} role="none">
                    <button
                      type="button"
                      role="menuitemradio"
                      aria-checked={appearance === option.value}
                      className={
                        appearance === option.value
                          ? 'workspace-user-appearance-option active'
                          : 'workspace-user-appearance-option'
                      }
                      onClick={() => selectAppearance(option.value)}
                    >
                      <span>{option.label}</span>
                      {appearance === option.value && (
                        <CheckIcon
                          className="workspace-user-appearance-check"
                          aria-hidden
                        />
                      )}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </li>
        </ul>
      </div>

      <div className="workspace-user-popover-footer">
        <p className="workspace-user-encrypted">
          <LockIcon />
          <span>Cifrato end-to-end</span>
        </p>
        <button
          type="button"
          role="menuitem"
          className="workspace-user-logout"
          onClick={() => {
            onClose?.()
            onLogout()
          }}
        >
          <LogOutIcon className="workspace-user-nav-icon" />
          Esci e cancella memoria
        </button>
      </div>
    </>
  )
}

const POPOVER_GAP_PX = 7

const popoverWidthFor = (anchorLeft: number): number =>
  Math.min(272, window.innerWidth - anchorLeft - 16, window.innerWidth - 32)

export const WorkspaceUserMenu = ({
  userLabel,
  projects,
  selectedProjectId,
  currentScreen,
  conflictCount,
  projectName,
  onProjectNameChange,
  onSelectProject,
  onCreateProject,
  onNavigate,
  onLogout,
  appearance,
  onAppearanceChange,
  variant = 'sidebar',
}: WorkspaceUserMenuProps) => {
  const [open, setOpen] = useState(false)
  const [portaledPopoverStyle, setPortaledPopoverStyle] = useState<CSSProperties>({})
  const rootRef = useRef<HTMLDivElement>(null)
  const triggerRef = useRef<HTMLButtonElement>(null)
  const popoverRef = useRef<HTMLDivElement>(null)
  const menuId = useId()
  const usePortaledPopover = variant === 'sidebar'

  useLayoutEffect(() => {
    if (!open || !usePortaledPopover || !rootRef.current || !triggerRef.current) {
      return
    }

    const updatePosition = () => {
      if (!rootRef.current || !triggerRef.current) return

      const menuRect = rootRef.current.getBoundingClientRect()
      const triggerRect = triggerRef.current.getBoundingClientRect()
      const width = popoverWidthFor(menuRect.left)

      setPortaledPopoverStyle({
        left: menuRect.left,
        bottom: window.innerHeight - triggerRect.top + POPOVER_GAP_PX,
        width,
        maxHeight: Math.max(
          120,
          Math.min(
            window.innerHeight * 0.7,
            triggerRect.top - POPOVER_GAP_PX - 16,
          ),
        ),
      })
    }

    updatePosition()
    window.addEventListener('resize', updatePosition)
    window.addEventListener('scroll', updatePosition, true)
    return () => {
      window.removeEventListener('resize', updatePosition)
      window.removeEventListener('scroll', updatePosition, true)
    }
  }, [open, usePortaledPopover])

  useEffect(() => {
    if (!open || variant === 'overview') return
    const onPointerDown = (event: MouseEvent) => {
      const target = event.target as Node
      if (
        !rootRef.current?.contains(target) &&
        !popoverRef.current?.contains(target)
      ) {
        setOpen(false)
      }
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false)
    }
    document.addEventListener('mousedown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('mousedown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
    }
  }, [open, variant])

  const panelProps = {
    projects,
    selectedProjectId,
    currentScreen,
    conflictCount,
    projectName,
    onProjectNameChange,
    onSelectProject,
    onCreateProject,
    onNavigate,
    onLogout,
    appearance,
    onAppearanceChange,
  }

  if (variant === 'overview') {
    return (
      <div
        className="workspace-user-menu workspace-user-menu--overview"
        ref={rootRef}
      >
        <div className="workspace-user-panel" aria-label="Account e impostazioni">
          <WorkspaceUserPanel {...panelProps} />
        </div>
      </div>
    )
  }

  return (
    <div
      className={
        variant === 'sidebar'
          ? 'workspace-user-menu workspace-user-menu--sidebar'
          : 'workspace-user-menu workspace-user-menu--compact'
      }
      ref={rootRef}
    >
      <button
        ref={triggerRef}
        type="button"
        className="workspace-user-trigger"
        aria-expanded={open}
        aria-haspopup="menu"
        aria-controls={menuId}
        onClick={() => setOpen((value) => !value)}
      >
        <span className="workspace-user-avatar" aria-hidden>
          {initialsFor(userLabel)}
        </span>
        <span className="workspace-user-label">{displayLabelFor(userLabel)}</span>
        <ChevronUpIcon className={open ? 'workspace-user-chevron open' : 'workspace-user-chevron'} />
      </button>

      {open &&
        (usePortaledPopover ? (
          createPortal(
            <div
              ref={popoverRef}
              id={menuId}
              className="workspace-user-popover workspace-user-popover--portaled"
              style={portaledPopoverStyle}
              role="menu"
              aria-label="Account e impostazioni"
            >
              <WorkspaceUserPanel
                {...panelProps}
                onClose={() => setOpen(false)}
              />
            </div>,
            document.body,
          )
        ) : (
          <div
            ref={popoverRef}
            id={menuId}
            className="workspace-user-popover"
            role="menu"
            aria-label="Account e impostazioni"
          >
            <WorkspaceUserPanel
              {...panelProps}
              onClose={() => setOpen(false)}
            />
          </div>
        ))}
    </div>
  )
}
