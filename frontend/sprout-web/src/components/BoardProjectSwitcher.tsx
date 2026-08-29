import { useEffect, useId, useRef, useState, type FormEvent } from 'react'
import type { ProjectItem } from '../store/app-store'
import { CheckIcon, FolderIcon, PlusIcon } from './icons'

const projectLabelFor = (item: ProjectItem): string =>
  item.document?.name ?? `Locked ${item.wire.id.slice(0, 8)}`

export interface BoardProjectSwitcherProps {
  projects: ProjectItem[]
  selectedProjectId?: string
  currentProject?: ProjectItem
  projectName: string
  onProjectNameChange(name: string): void
  onSelectProject(projectId: string): void
  onCreateProject(event: FormEvent): void
  /** When false, hides the create-project entry in the menu. */
  allowCreate?: boolean
}

export const BoardProjectSwitcher = ({
  projects,
  selectedProjectId,
  currentProject,
  projectName,
  onProjectNameChange,
  onSelectProject,
  onCreateProject,
  allowCreate = true,
}: BoardProjectSwitcherProps) => {
  const [open, setOpen] = useState(false)
  const [showCreate, setShowCreate] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)
  const menuId = useId()
  const selectedProject =
    currentProject ??
    projects.find((item) => item.wire.id === selectedProjectId)
  const displayName = selectedProject
    ? projectLabelFor(selectedProject)
    : 'Seleziona progetto'

  useEffect(() => {
    if (!open) return
    const onPointerDown = (event: MouseEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) {
        setOpen(false)
        setShowCreate(false)
      }
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setOpen(false)
        setShowCreate(false)
      }
    }
    document.addEventListener('mousedown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('mousedown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
    }
  }, [open])

  const select = (projectId: string) => {
    onSelectProject(projectId)
    setOpen(false)
    setShowCreate(false)
  }

  const submitCreate = (event: FormEvent) => {
    void onCreateProject(event)
    setShowCreate(false)
    setOpen(false)
  }

  return (
    <div className="board-project-switcher" ref={rootRef}>
      <button
        type="button"
        className="board-project-switcher-trigger"
        aria-expanded={open}
        aria-haspopup="menu"
        aria-controls={menuId}
        aria-label={`Progetto: ${displayName}`}
        onClick={() =>
          setOpen((value) => {
            if (value) setShowCreate(false)
            return !value
          })
        }
      >
        <FolderIcon />
        <span className="board-project-switcher-label">{displayName}</span>
      </button>
      {open && (
        <div
          id={menuId}
          className="board-project-switcher-menu"
          role="menu"
          aria-label="Progetti"
        >
          {projects.map((item) => (
            <button
              type="button"
              key={item.wire.id}
              role="menuitemradio"
              aria-checked={item.wire.id === selectedProjectId}
              className={
                item.wire.id === selectedProjectId
                  ? 'board-project-switcher-option active'
                  : 'board-project-switcher-option'
              }
              onClick={() => select(item.wire.id)}
            >
              <span>{projectLabelFor(item)}</span>
              {item.wire.id === selectedProjectId && (
                <CheckIcon
                  className="board-project-switcher-check"
                  aria-hidden
                />
              )}
            </button>
          ))}
          {allowCreate &&
            (showCreate ? (
              <form
                className="board-project-switcher-create-form"
                onSubmit={submitCreate}
              >
                <input
                  required
                  autoFocus
                  placeholder="Nome privato"
                  value={projectName}
                  onChange={(event) => onProjectNameChange(event.target.value)}
                  aria-label="Nome nuovo progetto"
                />
                <div className="board-project-switcher-create-actions">
                  <button type="submit" className="secondary-button">
                    Crea
                  </button>
                  <button
                    type="button"
                    className="text-button"
                    onClick={() => setShowCreate(false)}
                  >
                    Annulla
                  </button>
                </div>
              </form>
            ) : (
              <button
                type="button"
                role="menuitem"
                className="board-project-switcher-create-trigger"
                onClick={() => setShowCreate(true)}
              >
                <PlusIcon aria-hidden />
                <span>Nuovo progetto</span>
              </button>
            ))}
        </div>
      )}
    </div>
  )
}
