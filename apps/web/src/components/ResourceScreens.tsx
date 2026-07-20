import { useState, type FormEvent } from 'react'
import type {
  AttachmentCollectionItemDto,
  ParticipantSuggestionDto,
  PermissionGrantDto,
  ProjectInvitationDto,
  ProjectRecoveryStatus,
  RetentionArchiveDto,
  RetentionWarningDto,
  Uuid,
} from '../api/contracts'
import type { DecryptedTask, SyncConflict } from '../domain/models'
import type { VaultPersistence } from '../security/key-vault'
import {
  DownloadIcon,
  KeyIcon,
  LockIcon,
  ShieldIcon,
} from './icons'

interface ProjectPeopleScreenProps {
  invitations: ProjectInvitationDto[]
  suggestions: ParticipantSuggestionDto[]
  onRefresh(): Promise<void>
  onInvite(input: {
    email: string
    name: string
    phone?: string
    role: 'admin' | 'member' | 'guest'
  }): Promise<void>
  onAccept(input: {
    projectId: Uuid
    invitationId: Uuid
    token: string
  }): Promise<void>
  onShare(identityId: Uuid): Promise<void>
  managedGrants: Array<{
    topicName: string
    resourceId: Uuid
    grant: PermissionGrantDto
  }>
  onRevoke(input: {
    resourceId: Uuid
    grantId: Uuid
    userId: Uuid
  }): Promise<void>
  onSuggest(prefix: string): Promise<void>
}

export const ProjectPeopleScreen = ({
  invitations,
  suggestions,
  onRefresh,
  onInvite,
  onAccept,
  onShare,
  managedGrants,
  onRevoke,
  onSuggest,
}: ProjectPeopleScreenProps) => {
  const [email, setEmail] = useState('')
  const [name, setName] = useState('')
  const [phone, setPhone] = useState('')
  const [role, setRole] = useState<'admin' | 'member' | 'guest'>('member')
  const [acceptProjectId, setAcceptProjectId] = useState('')
  const [invitationId, setInvitationId] = useState('')
  const [token, setToken] = useState('')
  const [prefix, setPrefix] = useState('')

  return (
    <section className="content-screen" aria-labelledby="people-title">
      <div className="screen-heading">
        <div>
          <p className="eyebrow">Project access</p>
          <h2 id="people-title">Participants and invitations</h2>
        </div>
        <button
          className="secondary-button inline-button"
          type="button"
          onClick={() => void onRefresh()}
        >
          Refresh invitations
        </button>
      </div>
      <div className="resource-grid">
        <form
          className="panel-form"
          onSubmit={(event) => {
            event.preventDefault()
            void onInvite({
              email,
              name,
              phone: phone || undefined,
              role,
            }).then(() => {
              setEmail('')
              setName('')
              setPhone('')
            })
          }}
        >
          <h3>Invite a participant</h3>
          <p>
            Email remains visible for delivery. Name and phone are encrypted
            locally before leaving this device.
          </p>
          <label>
            Email
            <input
              type="email"
              required
              value={email}
              onChange={(event) => setEmail(event.target.value)}
            />
          </label>
          <label>
            Name
            <input
              required
              value={name}
              onChange={(event) => setName(event.target.value)}
            />
          </label>
          <label>
            Phone
            <input
              type="tel"
              value={phone}
              onChange={(event) => setPhone(event.target.value)}
            />
          </label>
          <label>
            Role
            <select
              value={role}
              onChange={(event) =>
                setRole(event.target.value as 'admin' | 'member' | 'guest')
              }
            >
              <option value="member">Member</option>
              <option value="guest">Guest</option>
              <option value="admin">Admin</option>
            </select>
          </label>
          <button className="primary-button" type="submit">
            Encrypt and invite
          </button>
        </form>

        <form
          className="panel-form"
          onSubmit={(event) => {
            event.preventDefault()
            void onAccept({
              projectId: acceptProjectId,
              invitationId,
              token,
            })
          }}
        >
          <h3>Accept an invitation</h3>
          <label>
            Project ID
            <input
              required
              pattern="[0-9a-fA-F-]{36}"
              value={acceptProjectId}
              onChange={(event) => setAcceptProjectId(event.target.value)}
            />
          </label>
          <label>
            Invitation ID
            <input
              required
              pattern="[0-9a-fA-F-]{36}"
              value={invitationId}
              onChange={(event) => setInvitationId(event.target.value)}
            />
          </label>
          <label>
            Email token
            <input
              required
              minLength={64}
              maxLength={64}
              value={token}
              onChange={(event) => setToken(event.target.value)}
            />
          </label>
          <button className="secondary-button" type="submit">
            Accept
          </button>
        </form>

        <form
          className="panel-form"
          onSubmit={(event) => {
            event.preventDefault()
            void onSuggest(prefix)
          }}
        >
          <h3>Known participants</h3>
          <label>
            Identity handle prefix
            <input
              maxLength={128}
              value={prefix}
              onChange={(event) => setPrefix(event.target.value)}
            />
          </label>
          <button className="secondary-button" type="submit">
            Rank shared participants
          </button>
          <ul className="archive-list">
            {suggestions.map((suggestion) => (
              <li key={suggestion.identity_id}>
                <div>
                  <strong>{suggestion.identity_handle}</strong>
                  <small>{suggestion.identity_id}</small>
                </div>
                <span>{suggestion.shared_project_count} shared projects</span>
              </li>
            ))}
          </ul>
        </form>
      </div>

      <ul className="archive-list">
        {invitations.map((invitation) => (
          <li key={invitation.id}>
            <div>
              <strong>{invitation.role}</strong>
              <small>{invitation.id}</small>
              {invitation.accepted_by_identity_id && (
                <small>{invitation.accepted_by_identity_id}</small>
              )}
            </div>
            <div>
              <span>{invitation.state}</span>
              {invitation.keys_shared ? (
                <small>Encrypted access shared</small>
              ) : (
                invitation.state === 'accepted' &&
                invitation.accepted_by_identity_id && (
                  <button
                    className="secondary-button inline-button"
                    type="button"
                    onClick={() =>
                      void onShare(invitation.accepted_by_identity_id as Uuid)
                    }
                  >
                    Share encrypted project
                  </button>
                )
              )}
            </div>
          </li>
        ))}
      </ul>

      <div className="screen-heading">
        <div>
          <p className="eyebrow">Encrypted resource grants</p>
          <h3>Managed topic access</h3>
        </div>
      </div>
      <ul className="archive-list">
        {managedGrants.map(({ topicName, resourceId, grant }) => (
          <li key={grant.id}>
            <div>
              <strong>{topicName}</strong>
              <small>{grant.user_id}</small>
              <small>
                {grant.access_level} · {grant.access_scope}
              </small>
            </div>
            <button
              className="secondary-button inline-button"
              type="button"
              onClick={() =>
                void onRevoke({
                  resourceId,
                  grantId: grant.id,
                  userId: grant.user_id,
                })
              }
            >
              Revoke and rotate keys
            </button>
          </li>
        ))}
      </ul>
    </section>
  )
}

interface ResourceLookupProps {
  kind: 'preset' | 'questionnaire'
  onCreate(name: string): Promise<void>
  onOpen(id: Uuid): Promise<void>
  result?: { id: Uuid; name?: string; locked?: boolean; detail?: string }
}

export const ResourceLookupScreen = ({
  kind,
  onCreate,
  onOpen,
  result,
}: ResourceLookupProps) => {
  const [name, setName] = useState('')
  const [id, setId] = useState('')
  const title = kind === 'preset' ? 'Presets' : 'Questionnaires'

  const create = async (event: FormEvent) => {
    event.preventDefault()
    await onCreate(name)
    setName('')
  }
  const open = async (event: FormEvent) => {
    event.preventDefault()
    await onOpen(id)
  }

  return (
    <section className="content-screen" aria-labelledby={`${kind}-title`}>
      <div className="screen-heading">
        <div>
          <p className="eyebrow">Encrypted resource</p>
          <h2 id={`${kind}-title`}>{title}</h2>
        </div>
      </div>
      <div className="resource-grid">
        <form className="panel-form" onSubmit={(event) => void create(event)}>
          <h3>Create {kind}</h3>
          <p>
            The name is encrypted locally before the current API route receives
            it.
          </p>
          <label>
            Private name
            <input
              required
              value={name}
              onChange={(event) => setName(event.target.value)}
            />
          </label>
          <button className="primary-button" type="submit">
            Encrypt and create
          </button>
        </form>

        <form className="panel-form" onSubmit={(event) => void open(event)}>
          <h3>Open by ID</h3>
          <p>
            The server currently has no list route for {title.toLowerCase()}.
          </p>
          <label>
            Resource ID
            <input
              required
              pattern="[0-9a-fA-F-]{36}"
              value={id}
              onChange={(event) => setId(event.target.value)}
            />
          </label>
          <button className="secondary-button" type="submit">
            Load encrypted resource
          </button>
        </form>
      </div>

      {result && (
        <div className="result-card" role="status">
          {result.locked ? <LockIcon /> : <ShieldIcon />}
          <div>
            <strong>{result.name ?? `Locked ${kind}`}</strong>
            <p>{result.detail ?? result.id}</p>
          </div>
        </div>
      )}
    </section>
  )
}

interface AttachmentScreenProps {
  assigneeTasks: DecryptedTask[]
  attachments: AttachmentCollectionItemDto[]
  onRefresh(taskId: Uuid): Promise<void>
  onUpload(
    task: DecryptedTask,
    file: File,
    requiredAttachmentId?: Uuid,
  ): Promise<void>
  onResume(attachment: AttachmentCollectionItemDto): Promise<void>
  onDownload(attachment: AttachmentCollectionItemDto): Promise<void>
}

export const AttachmentScreen = ({
  assigneeTasks,
  attachments,
  onRefresh,
  onUpload,
  onResume,
  onDownload,
}: AttachmentScreenProps) => {
  const [taskId, setTaskId] = useState('')
  const [file, setFile] = useState<File>()
  const [requiredAttachmentId, setRequiredAttachmentId] = useState('')
  const selectedTask = assigneeTasks.find((task) => task.wire.id === taskId)
  const requiredAttachments = attachments.filter(
    (attachment) =>
      attachment.task_id === taskId &&
      attachment.attachment_kind === 'task_required',
  )

  return (
    <section className="content-screen" aria-labelledby="attachment-title">
      <div className="screen-heading">
        <div>
          <p className="eyebrow">Ciphertext files</p>
          <h2 id="attachment-title">Attachments</h2>
        </div>
      </div>
      <div className="resource-grid">
        <form
          className="panel-form"
          onSubmit={(event) => {
            event.preventDefault()
            if (!selectedTask || !file) return
            const form = event.currentTarget
            void onUpload(
              selectedTask,
              file,
              requiredAttachmentId || undefined,
            ).then(() => {
              setFile(undefined)
              form.reset()
            })
          }}
        >
          <h3>Complete with an encrypted file</h3>
          <p>
            The browser encrypts bytes and private metadata before declaration.
            Local paths and plaintext names never enter the request.
          </p>
          <label>
            Active assignment
            <select
              required
              value={taskId}
              onChange={(event) => {
                const nextTaskId = event.target.value
                setTaskId(nextTaskId)
                setRequiredAttachmentId('')
                if (nextTaskId) void onRefresh(nextTaskId)
              }}
            >
              <option value="">Choose an assigned task</option>
              {assigneeTasks.map((task) => (
                <option key={task.wire.id} value={task.wire.id}>
                  {task.document.title}
                </option>
              ))}
            </select>
          </label>
          {requiredAttachments.length > 0 && (
            <label>
              Required attachment being completed
              <select
                value={requiredAttachmentId}
                onChange={(event) =>
                  setRequiredAttachmentId(event.target.value)
                }
              >
                <option value="">Additional attachment</option>
                {requiredAttachments.map((attachment) => (
                  <option key={attachment.id} value={attachment.id}>
                    Required · {attachment.blob_id}
                  </option>
                ))}
              </select>
            </label>
          )}
          <label>
            Local file
            <input
              required
              type="file"
              onChange={(event) => setFile(event.target.files?.[0])}
            />
          </label>
          <button
            type="submit"
            className="primary-button"
            disabled={!selectedTask || !file}
          >
            Encrypt, persist, and upload
          </button>
        </form>
        <div className="panel-form">
          <h3>Ciphertext-only local storage</h3>
          <p>
            OPFS stores only the opaque blob ID and encrypted container. A
            readable download is created only after this device proves it has
            the task resource key.
          </p>
          <button
            type="button"
            className="secondary-button"
            disabled={!taskId}
            onClick={() => taskId && void onRefresh(taskId)}
          >
            Refresh task files
          </button>
        </div>
      </div>
      {attachments.length === 0 ? (
        <div className="screen-empty compact-empty">
          <h3>No task attachments loaded</h3>
          <p>
            Choose one of your active assignments. Declarations are bound to
            its task resource, active key epoch, and assignment ID.
          </p>
        </div>
      ) : (
        <ul className="archive-list attachment-list">
          {attachments.map((attachment) => (
            <li key={attachment.id}>
              <div>
                <strong>
                  {attachment.attachment_kind === 'task_required'
                    ? 'Required encrypted attachment'
                    : 'Completed encrypted attachment'}
                </strong>
                <small>{attachment.blob_id}</small>
              </div>
              <span>{attachment.state.state.replaceAll('_', ' ')}</span>
              {attachment.state.state === 'pending_upload' ? (
                <button
                  type="button"
                  className="secondary-button"
                  onClick={() => void onResume(attachment)}
                >
                  Resume encrypted upload
                </button>
              ) : (
                <button
                  type="button"
                  className="secondary-button"
                  disabled={attachment.state.state !== 'available'}
                  onClick={() => void onDownload(attachment)}
                >
                  <DownloadIcon />
                  Decrypt to download
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}

interface RetentionScreenProps {
  autoExport?: boolean
  archives: RetentionArchiveDto[]
  warnings: RetentionWarningDto[]
  onToggle(value: boolean): Promise<void>
  onRefresh(): Promise<void>
  onDownload(archive: RetentionArchiveDto): Promise<void>
}

export const RetentionScreen = ({
  autoExport,
  archives,
  warnings,
  onToggle,
  onRefresh,
  onDownload,
}: RetentionScreenProps) => (
  <section className="content-screen" aria-labelledby="retention-title">
    <div className="screen-heading">
      <div>
        <p className="eyebrow">Deletion and export</p>
        <h2 id="retention-title">Retention archives</h2>
      </div>
      <button
        className="secondary-button inline-button"
        type="button"
        onClick={() => void onRefresh()}
      >
        Refresh
      </button>
    </div>
    {warnings.length > 0 && (
      <div className="retention-preference" role="status">
        <div>
          <h3>Retention warning</h3>
          <p>
            {warnings.length} retained resource{warnings.length === 1 ? '' : 's'}{' '}
            reached a deletion warning window. Prepare an encrypted export if
            needed.
          </p>
        </div>
      </div>
    )}
    <div className="retention-preference">
      <div>
        <h3>Prepare encrypted exports before purge</h3>
        <p>
          Availability is not automatic: completed archives still require a
          user-initiated forced download.
        </p>
      </div>
      <label className="switch-label">
        <input
          type="checkbox"
          checked={Boolean(autoExport)}
          onChange={(event) => void onToggle(event.target.checked)}
        />
        Auto-prepare
      </label>
    </div>
    {archives.length === 0 ? (
      <div className="screen-empty compact-empty">
        <h3>No archives available</h3>
        <p>The API returned no active retention exports.</p>
      </div>
    ) : (
      <ul className="archive-list">
        {archives.map((archive) => (
          <li key={archive.id}>
            <div>
              <strong>{archive.source_kind}</strong>
              <small>{archive.state}</small>
            </div>
            <span>{new Date(archive.created_at).toLocaleString()}</span>
            <button
              type="button"
              className="secondary-button"
              disabled={archive.state !== 'succeeded'}
              onClick={() => void onDownload(archive)}
            >
              <DownloadIcon />
              Download ciphertext
            </button>
          </li>
        ))}
      </ul>
    )}
  </section>
)

interface RecoveryScreenProps {
  projectId?: Uuid
  status?: ProjectRecoveryStatus
  onProvision(): Promise<void>
  onStart(kind: 'participant_device' | 'lost_owner'): Promise<void>
  onLoad(requestId: Uuid): Promise<void>
  onApprove(input: {
    requestId: Uuid
    encryptedShareB64: string
    keyVersion: number
  }): Promise<void>
  onCombine(shares: string[]): Promise<void>
}

export const RecoveryScreen = ({
  projectId,
  status,
  onProvision,
  onStart,
  onLoad,
  onApprove,
  onCombine,
}: RecoveryScreenProps) => {
  const [requestId, setRequestId] = useState('')
  const [share, setShare] = useState('')
  const [keyVersion, setKeyVersion] = useState('1')
  const [shares, setShares] = useState('')
  const approved = status?.approved_approver_ids.length ?? 0
  const required = status?.required_approver_ids.length ?? 0
  const canFinalize =
    Boolean(status?.delivery_available) &&
    approved > 0 &&
    approved === required

  if (!projectId) {
    return (
      <section className="screen-empty">
        <h2>Select a project for recovery</h2>
        <p>Recovery electorate and approvals are project-scoped.</p>
      </section>
    )
  }

  return (
    <section className="content-screen" aria-labelledby="recovery-title">
      <div className="screen-heading">
        <div>
          <p className="eyebrow">Unanimous recovery</p>
          <h2 id="recovery-title">Recovery ceremony</h2>
        </div>
      </div>
      <div className="recovery-summary">
        <ShieldIcon />
        <div>
          <strong>
            {status ? `${approved} of ${required} approved` : 'No request loaded'}
          </strong>
          <p>
            Every frozen electorate member must approve. One missing share
            prevents recovery by design. Owner-only projects cannot recover the
            owner.
          </p>
        </div>
        {status && (
          <progress
            max={Math.max(required, 1)}
            value={approved}
            aria-label={`${approved} of ${required} approvals`}
          />
        )}
      </div>
      <div className="resource-grid recovery-grid">
        <div className="panel-form">
          <h3>Provision shares</h3>
          <p>
            After membership changes, provision a new n-of-n set. Existing
            projects stay unrecoverable until this step succeeds.
          </p>
          <button
            type="button"
            className="secondary-button"
            onClick={() => void onProvision()}
          >
            Provision and activate recovery
          </button>
        </div>
        <div className="panel-form">
          <h3>Start request</h3>
          <button
            type="button"
            className="secondary-button"
            onClick={() => void onStart('participant_device')}
          >
            Recover participant device
          </button>
          <button
            type="button"
            className="secondary-button"
            onClick={() => void onStart('lost_owner')}
          >
            Start lost-owner recovery
          </button>
        </div>
        <form
          className="panel-form"
          onSubmit={(event) => {
            event.preventDefault()
            void onLoad(requestId)
          }}
        >
          <h3>Load status</h3>
          <label>
            Request ID
            <input
              required
              pattern="[0-9a-fA-F-]{36}"
              value={requestId}
              onChange={(event) => setRequestId(event.target.value)}
            />
          </label>
          <button type="submit" className="secondary-button">
            Load request
          </button>
        </form>
        <form
          className="panel-form"
          onSubmit={(event) => {
            event.preventDefault()
            void onApprove({
              requestId,
              encryptedShareB64: share,
              keyVersion: Number(keyVersion),
            })
          }}
        >
          <h3>Approve with dual signatures</h3>
          <label>
            Encrypted share override (optional)
            <textarea
              value={share}
              onChange={(event) => setShare(event.target.value)}
              placeholder="Leave empty to unwrap this device's provisioned share"
            />
          </label>
          <label>
            Device key version
            <input
              type="number"
              min="1"
              required
              value={keyVersion}
              onChange={(event) => setKeyVersion(event.target.value)}
            />
          </label>
          <p>
            Load status first. Empty share field uses the active provisioned
            share for this device.
          </p>
          <button type="submit" className="secondary-button">
            Sign and approve
          </button>
        </form>
        <form
          className="panel-form"
          onSubmit={(event) => {
            event.preventDefault()
            void onCombine(
              shares
                .split(/\s+/)
                .map((value) => value.trim())
                .filter(Boolean),
            )
          }}
        >
          <h3>Combine and finalize</h3>
          <label>
            Manual share override (optional)
            <textarea
              value={shares}
              onChange={(event) => setShares(event.target.value)}
              placeholder="Leave empty when requester deliveries are available"
            />
          </label>
          <button
            type="submit"
            className="secondary-button"
            disabled={!canFinalize && shares.trim().length === 0}
          >
            Combine shares and finalize recovery
          </button>
        </form>
      </div>
    </section>
  )
}

interface SecurityScreenProps {
  vaultPersistence: VaultPersistence
  storagePersistence: 'unknown' | 'granted' | 'not-granted'
  onRegisterPasskey(): Promise<void>
  onPersistStorage(): Promise<void>
}

export const SecurityScreen = ({
  vaultPersistence,
  storagePersistence,
  onRegisterPasskey,
  onPersistStorage,
}: SecurityScreenProps) => (
  <section className="content-screen" aria-labelledby="security-title">
    <div className="screen-heading">
      <div>
        <p className="eyebrow">Device security</p>
        <h2 id="security-title">Passkeys and local storage</h2>
      </div>
    </div>
    <div className="security-grid">
      <article>
        <KeyIcon />
        <h3>Passkey registration</h3>
        <p>
          Registration authenticates this device. If the authenticator returns
          PRF output, Sprout derives a non-exportable vault-wrapping key.
        </p>
        <strong>Vault: {vaultPersistence}</strong>
        <button
          type="button"
          className="secondary-button"
          onClick={() => void onRegisterPasskey()}
        >
          Register passkey
        </button>
      </article>
      <article>
        <LockIcon />
        <h3>Persistent encrypted storage</h3>
        <p>
          Persistence reduces browser eviction risk but does not prevent a user
          from clearing site data.
        </p>
        <strong>Storage: {storagePersistence}</strong>
        <button
          type="button"
          className="secondary-button"
          onClick={() => void onPersistStorage()}
        >
          Request persistence
        </button>
      </article>
      <article className="limitations-card">
        <ShieldIcon />
        <h3>Security limitations</h3>
        <ul>
          <li>A compromised origin can steal keys while unlocked.</li>
          <li>Traffic timing, sizes, membership, and identifiers remain visible.</li>
          <li>Downloaded files leave browser-managed encrypted storage.</li>
          <li>PRF and OPFS support varies; only executed browsers are known.</li>
        </ul>
      </article>
    </div>
  </section>
)

interface ConflictScreenProps {
  conflicts: SyncConflict[]
  onDiscard(conflict: SyncConflict): Promise<void>
  onRetry(conflict: SyncConflict): Promise<void>
}

export const ConflictScreen = ({
  conflicts,
  onDiscard,
  onRetry,
}: ConflictScreenProps) => (
  <section className="content-screen" aria-labelledby="conflicts-title">
    <div className="screen-heading">
      <div>
        <p className="eyebrow">Client-side resolution</p>
        <h2 id="conflicts-title">Encrypted conflicts</h2>
      </div>
    </div>
    {conflicts.length === 0 ? (
      <div className="screen-empty compact-empty">
        <h3>No unresolved conflicts</h3>
        <p>REST cursor sync has not produced divergent encrypted versions.</p>
      </div>
    ) : (
      <ul className="conflict-list">
        {conflicts.map((conflict) => (
          <li key={conflict.id}>
            <div>
              <strong>{conflict.reason.replaceAll('-', ' ')}</strong>
              <p>{conflict.resourceId}</p>
            </div>
            <span>
              Remote version {conflict.remoteVersion ?? 'not supplied'}
            </span>
            <div>
              <button
                type="button"
                className="secondary-button"
                onClick={() => void onDiscard(conflict)}
              >
                Keep remote
              </button>
              <button
                type="button"
                className="secondary-button"
                disabled={conflict.remoteVersion === undefined}
                onClick={() => void onRetry(conflict)}
              >
                Re-encrypt local resolution
              </button>
            </div>
          </li>
        ))}
      </ul>
    )}
  </section>
)
