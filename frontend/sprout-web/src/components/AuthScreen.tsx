import { useState, type FormEvent } from 'react'
import type { Uuid } from '../api/contracts'
import { KeyIcon, LockIcon, SproutIcon } from './icons'

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
  }): Promise<void>
  onVerify(input: {
    identityId: Uuid
    token: string
  }): Promise<void>
  onRecoveryStart(email: string): Promise<void>
  onRecoveryFinish(input: {
    identityId: Uuid
    token: string
  }): Promise<void>
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
}: AuthScreenProps) => {
  const [mode, setMode] = useState<AuthMode>('signin')
  const [verificationKind, setVerificationKind] = useState<
    'signup' | 'recovery'
  >('signup')
  const [email, setEmail] = useState('')
  const [identityHandle, setIdentityHandle] = useState('')
  const [identityId, setIdentityId] = useState('')
  const [token, setToken] = useState('')

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (mode === 'signin') {
      await onSignIn({ identityId, identityHandle })
      return
    }
    if (mode === 'signup') {
      await onSignup({ email, identityHandle })
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

  return (
    <main className="auth-page">
      <section className="auth-intro" aria-labelledby="auth-title">
        <a className="brand auth-brand" href="/" aria-label="Sprout home">
          <span className="brand-mark">
            <SproutIcon />
          </span>
          <span>Sprout</span>
        </a>
        <p className="eyebrow">Encrypted workspace</p>
        <h1 id="auth-title">Your work stays readable only on authorized devices.</h1>
        <p>
          Sprout sends ciphertext and routing metadata to the service. Private
          keys and decrypted content remain in this browser&apos;s memory.
        </p>
        <ul className="auth-assurances">
          <li>
            <LockIcon />
            Rust/WASM encryption with resource and version-bound context
          </li>
          <li>
            <KeyIcon />
            Passkeys authenticate; PRF may wrap a local key vault
          </li>
        </ul>
        <div className="security-disclosure">
          <strong>Passkey limitation</strong>
          <p>
            A passkey does not reveal encryption keys. Without WebAuthn PRF and
            this device&apos;s wrapped vault, another authorized device or
            unanimous project recovery is required.
          </p>
        </div>
      </section>

      <section className="auth-card" aria-labelledby="form-title">
        <div className="auth-tabs" role="tablist" aria-label="Account access">
          <button
            type="button"
            role="tab"
            aria-selected={mode === 'signin'}
            onClick={() => setMode('signin')}
          >
            Sign in
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={mode === 'signup'}
            onClick={() => setMode('signup')}
          >
            Create account
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={mode === 'recover'}
            onClick={() => setMode('recover')}
          >
            Recover
          </button>
        </div>

        <form
          onSubmit={(event) => {
            void submit(event).catch(() => undefined)
          }}
        >
          <h2 id="form-title">
            {mode === 'signin' && 'Sign in with a passkey'}
            {mode === 'signup' && 'Create your encrypted account'}
            {mode === 'recover' && 'Request account recovery'}
            {mode === 'verify' &&
              (verificationKind === 'signup'
                ? 'Verify your email'
                : 'Finish account recovery')}
          </h2>

          {(mode === 'signup' || mode === 'recover') && (
            <label>
              Email
              <input
                type="email"
                autoComplete="email"
                required
                value={email}
                onChange={(event) => setEmail(event.target.value)}
              />
            </label>
          )}

          {(mode === 'signin' || mode === 'signup') && (
            <label>
              Identity handle
              <input
                type="text"
                minLength={3}
                maxLength={128}
                autoComplete="username"
                required
                value={identityHandle}
                onChange={(event) => setIdentityHandle(event.target.value)}
              />
            </label>
          )}

          {(mode === 'signin' || mode === 'verify') && (
            <label>
              Identity ID
              <input
                type="text"
                inputMode="text"
                required
                pattern="[0-9a-fA-F-]{36}"
                value={identityId}
                onChange={(event) => setIdentityId(event.target.value)}
              />
            </label>
          )}

          {mode === 'verify' && (
            <label>
              Email token
              <input
                type="text"
                minLength={64}
                maxLength={64}
                autoComplete="one-time-code"
                required
                value={token}
                onChange={(event) => setToken(event.target.value)}
              />
            </label>
          )}

          {!online && (
            <p className="form-message warning" role="status">
              Account ceremonies require a network connection.
            </p>
          )}
          {error && (
            <p className="form-message error" role="alert">
              {error}
            </p>
          )}
          {notice && (
            <p className="form-message" role="status">
              {notice}
            </p>
          )}

          <button
            className="primary-button auth-submit"
            type="submit"
            disabled={busy || !online}
          >
            {busy ? 'Working…' : mode === 'signin' ? 'Use passkey' : 'Continue'}
          </button>
        </form>

        {offlineVaultAvailable && (
          <div className="local-unlock">
            <hr />
            <h3>Open this device offline</h3>
            <p>
              A fresh local WebAuthn challenge requests PRF output from the
              passkey bound to this encrypted vault. This does not create a
              server session.
            </p>
            <button
              className="secondary-button"
              type="button"
              disabled={busy}
              onClick={() => void onOfflineUnlock()}
            >
              Unlock local workspace
            </button>
          </div>
        )}

        <p className="device-footnote">
          Device routing ID: <code>{deviceId}</code>. It is metadata, not a
          decryption key.
        </p>
      </section>
    </main>
  )
}
