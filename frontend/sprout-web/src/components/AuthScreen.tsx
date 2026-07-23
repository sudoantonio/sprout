import { useState, type FormEvent } from 'react'
import type { EmailStartResponse, Uuid } from '../api/contracts'
import { KeyIcon, SproutIcon } from './icons'
import './AuthScreen.css'

type AuthMode = 'signin' | 'signup' | 'verify' | 'recover'

interface AuthScreenProps {
  online: boolean
  busy: boolean
  error?: string
  notice?: string
  deviceId: Uuid
  offlineVaultAvailable: boolean
  onOfflineUnlock(): Promise<void>
  onSignIn(input: {
    identityId: Uuid
    identityHandle: string
  }): Promise<void>
  onSignup(input: {
    email: string
    identityHandle: string
  }): Promise<EmailStartResponse>
  onVerify(input: {
    identityId: Uuid
    token: string
  }): Promise<void>
  onRecoveryStart(email: string): Promise<void>
  onRecoveryFinish(input: {
    identityId: Uuid
    token: string
  }): Promise<void>
  onDevLogin?(input: {
    email: string
    identityHandle: string
  }): Promise<void>
}

const DEV_EMAIL = 'admin@example.test'
const DEV_HANDLE = 'admin.minerva'

const titles: Record<AuthMode, string> = {
  signin: 'Accedi',
  signup: 'Crea account',
  recover: 'Recupera accesso',
  verify: 'Verifica email',
}

const hints: Record<AuthMode, string> = {
  signin:
    'Accedi con passkey funziona solo dopo verifica email e registrazione passkey. Se è la prima volta, usa Verifica email qui sotto.',
  signup: 'In sviluppo: «Continua» entra direttamente. La cifratura resta attiva.',
  recover: "Ti invieremo un token se l'account esiste.",
  verify:
    'Account già verificato? Usa Recupera. Altrimenti incolla il token completo (64 caratteri).',
}

export const AuthScreen = ({
  online,
  busy,
  error,
  notice,
  deviceId,
  offlineVaultAvailable,
  onOfflineUnlock,
  onSignIn,
  onSignup,
  onVerify,
  onRecoveryStart,
  onRecoveryFinish,
  onDevLogin,
}: AuthScreenProps) => {
  const [mode, setMode] = useState<AuthMode>(
    import.meta.env.DEV ? 'signup' : 'signin',
  )
  const [verificationKind, setVerificationKind] = useState<
    'signup' | 'recovery'
  >('signup')
  const [email, setEmail] = useState(import.meta.env.DEV ? DEV_EMAIL : '')
  const [identityHandle, setIdentityHandle] = useState(
    import.meta.env.DEV ? DEV_HANDLE : '',
  )
  const [identityId, setIdentityId] = useState('')
  const [token, setToken] = useState('')

  const switchMode = (next: AuthMode) => {
    setMode(next)
    if (next !== 'verify') setToken('')
  }

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (mode === 'signin') {
      await onSignIn({ identityId, identityHandle })
      return
    }
    if (mode === 'signup') {
      if (import.meta.env.DEV && onDevLogin) {
        await onDevLogin({ email, identityHandle })
        return
      }
      const response = await onSignup({ email, identityHandle })
      if (!response.dev_verification_token) {
        return
      }
      if (response.identity_id) {
        setIdentityId(response.identity_id)
      }
      setToken(response.dev_verification_token)
      setVerificationKind('signup')
      setMode('verify')
      return
    }
    if (mode === 'recover') {
      await onRecoveryStart(email)
      setVerificationKind('recovery')
      setMode('verify')
      return
    }
    if (verificationKind === 'signup') {
      await onVerify({ identityId, token })
    } else {
      await onRecoveryFinish({ identityId, token })
    }
  }

  const submitLabel = busy
    ? 'Attendere…'
    : mode === 'signin'
      ? 'Continua con passkey'
      : mode === 'verify'
        ? 'Verifica'
        : mode === 'signup' && import.meta.env.DEV
          ? 'Entra (dev)'
          : 'Continua'

  return (
    <main className="auth-page">
      <div className="auth-card">
        <header className="auth-brand">
          <a className="auth-brand-link" href="/" aria-label="Sprout">
            <span className="auth-brand-mark">
              <SproutIcon />
            </span>
            <span className="auth-brand-name">Sprout</span>
          </a>
          <p className="auth-brand-tagline">
            Workspace cifrato, solo sui tuoi device.
          </p>
        </header>

        <section className="auth-panel" aria-labelledby="form-title">
          {import.meta.env.DEV && onDevLogin && mode !== 'verify' && (
            <div className="auth-dev-login">
              <p>Sviluppo frontend: accesso rapido con cifratura attiva.</p>
              <button
                type="button"
                className="auth-dev-login-button"
                disabled={busy || !online}
                onClick={() =>
                  void onDevLogin({
                    email: email || DEV_EMAIL,
                    identityHandle: identityHandle || DEV_HANDLE,
                  })
                }
              >
                Entra come {identityHandle || DEV_HANDLE}
              </button>
            </div>
          )}

          {mode !== 'verify' && (
            <div className="auth-segment" role="tablist" aria-label="Accesso">
              <button
                type="button"
                role="tab"
                aria-selected={mode === 'signin'}
                onClick={() => switchMode('signin')}
              >
                Accedi
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={mode === 'signup'}
                onClick={() => switchMode('signup')}
              >
                Crea
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={mode === 'recover'}
                onClick={() => switchMode('recover')}
              >
                Recupera
              </button>
            </div>
          )}

          <form
            className="auth-form"
            onSubmit={(event) => {
              void submit(event).catch(() => undefined)
            }}
          >
            <div className="auth-form-head">
              <h1 id="form-title">
                {mode === 'verify'
                  ? verificationKind === 'signup'
                    ? 'Verifica email'
                    : 'Completa recupero'
                  : titles[mode]}
              </h1>
              <p>{mode === 'verify' ? hints.verify : hints[mode]}</p>
            </div>

            <div className="auth-fields">
              {(mode === 'signup' || mode === 'recover') && (
                <label className="auth-field">
                  Email
                  <input
                    type="email"
                    autoComplete="email"
                    required
                    placeholder="tu@esempio.test"
                    value={email}
                    onChange={(event) => setEmail(event.target.value)}
                  />
                </label>
              )}

              {(mode === 'signin' || mode === 'signup') && (
                <label className="auth-field">
                  Handle
                  <input
                    type="text"
                    minLength={3}
                    maxLength={128}
                    autoComplete="username"
                    required
                    placeholder="nome.cognome"
                    value={identityHandle}
                    onChange={(event) => setIdentityHandle(event.target.value)}
                  />
                </label>
              )}

              {(mode === 'signin' || mode === 'verify') && (
                <label className="auth-field">
                  Identity ID
                  <input
                    type="text"
                    inputMode="text"
                    required
                    pattern="[0-9a-fA-F-]{36}"
                    spellCheck={false}
                    placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
                    value={identityId}
                    onChange={(event) => setIdentityId(event.target.value)}
                  />
                </label>
              )}

              {mode === 'verify' && (
                <label className="auth-field auth-field--token">
                  Token email
                  <input
                    type="text"
                    minLength={64}
                    maxLength={64}
                    autoComplete="one-time-code"
                    required
                    spellCheck={false}
                    placeholder="64 caratteri esadecimali"
                    value={token}
                    onChange={(event) => setToken(event.target.value.trim())}
                  />
                  <span className="auth-field-hint">
                    {token.length}/64 caratteri
                  </span>
                </label>
              )}
            </div>

            {!online && (
              <p className="auth-message auth-message--warning" role="status">
                Serve connessione per accedere o creare un account.
              </p>
            )}
            {error && (
              <p className="auth-message auth-message--error" role="alert">
                {error}
              </p>
            )}
            {notice && (
              <p className="auth-message auth-message--neutral" role="status">
                {notice}
              </p>
            )}

            <button
              className="auth-primary"
              type="submit"
              disabled={busy || !online}
            >
              {mode === 'signin' && <KeyIcon />}
              {submitLabel}
            </button>

            {mode === 'signin' && (
              <button
                type="button"
                className="auth-ghost"
                onClick={() => {
                  setVerificationKind('signup')
                  setMode('verify')
                }}
              >
                Hai già creato l’account? Verifica email
              </button>
            )}

            {mode === 'verify' && (
              <>
                <details className="auth-dev">
                  <summary>Sviluppo: account già creato ma non verificato</summary>
                  <p>
                    Accedi con passkey funziona solo dopo verifica email e
                    registrazione passkey. Se hai già creato l’account, incolla
                    identity ID e token dall’outbox locale.
                  </p>
                  <pre>{`docker compose -f compose.validation.yml exec -T postgres \\
  psql -U sprout_validation -d sprout_validation -tA -F '|' \\
  -c "SELECT identity_id::text, encode(payload_nonce,'hex'), encode(encrypted_payload,'hex') FROM email_outbox WHERE recipient_email='TU@EMAIL' AND message_kind='signup_verification' ORDER BY created_at DESC LIMIT 1;" \\
| while IFS='|' read -r ID NONCE CT; do
  docker compose -f compose.validation.yml run --rm -T \\
    --entrypoint sprout-validation-crypto validation \\
    decrypt-email --identity-id "$ID" --message-kind signup_verification \\
    --nonce-hex "$NONCE" --ciphertext-hex "$CT"
done`}</pre>
                </details>
                <button
                  type="button"
                  className="auth-ghost"
                  onClick={() =>
                    switchMode(
                      verificationKind === 'signup' ? 'signup' : 'recover',
                    )
                  }
                >
                  Indietro
                </button>
              </>
            )}
          </form>

          {offlineVaultAvailable && (
            <div className="auth-offline">
              <p>Vault locale disponibile su questo device.</p>
              <button
                className="auth-secondary"
                type="button"
                disabled={busy}
                onClick={() => void onOfflineUnlock()}
              >
                Sblocca offline
              </button>
            </div>
          )}
        </section>

        <p className="auth-foot">
          Device <code>{deviceId.slice(0, 8)}</code>
        </p>
      </div>
    </main>
  )
}
