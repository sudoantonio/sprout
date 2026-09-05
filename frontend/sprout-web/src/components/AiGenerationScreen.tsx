import { useEffect, useMemo, useState } from 'react'
import type { KeyVault } from '../security/key-vault'
import {
  LOCAL_AI_PROFILE_NOTICE,
  type AiMode,
  type CommercialProvider,
  type LocalAiProfile,
  type SelfHostedEngine,
} from '../ai/contracts'
import { LocalAiProfileStore } from '../ai/profile'
import { providerForLocalProfile } from '../ai/providers'
import {
  OLLAMA_CHECKPOINT_MODEL,
  installOllamaWithConsent,
  pullOllamaModelWithSeparateConsent,
  type OllamaLifecycle,
} from '../ai/ollama-lifecycle'
import {
  browserDirectInferenceAllowed,
  resolveLocalEdgeInferenceBridge,
} from '../ai/execution-boundary'

const modes: Array<{ id: AiMode; label: string }> = [
  { id: 'commercial_api', label: 'A. API commerciale' },
  { id: 'lan_inference', label: 'B. Server di inferenza nella rete locale' },
  {
    id: 'private_remote',
    label: 'C. Server di inferenza remoto tramite connessione privata',
  },
  {
    id: 'commercial_privacy',
    label: 'D. API commerciale con protezione privacy locale',
  },
]

const providers: Array<{ id: CommercialProvider; label: string }> = [
  { id: 'openai', label: 'OpenAI' },
  { id: 'anthropic', label: 'Anthropic / Claude' },
  { id: 'xai', label: 'xAI / Grok' },
  { id: 'deepseek', label: 'DeepSeek' },
  { id: 'openai_compatible', label: 'OpenAI-compatible custom' },
  { id: 'anthropic_compatible', label: 'Anthropic-compatible custom' },
]

const defaultPreferences = {
  timeoutMs: 30_000,
  maxOutputTokens: 512,
  maxAttempts: 2,
}

const emptyProfile = (mode: AiMode): LocalAiProfile => {
  if (mode === 'lan_inference') {
    return {
      mode,
      engine: 'ollama',
      baseUrl: 'http://127.0.0.1:11434',
      model: '',
      preferences: defaultPreferences,
    }
  }
  if (mode === 'private_remote') {
    return {
      mode,
      engine: 'ds4',
      destination: '',
      baseUrl: 'https://',
      tlsPinSha256: '',
      validatedTransport: false,
      model: '',
      preferences: defaultPreferences,
    }
  }
  if (mode === 'commercial_privacy') {
    return {
      mode,
      provider: 'deepseek',
      credential: '',
      companionUrl: 'http://127.0.0.1',
      companionProtocolVersion: 'sprout-local-privacy-v1',
      privacyModel: 'gpt-oss-safeguard-20b',
      companionInstalled: false,
      modelInstalled: false,
      model: '',
      preferences: defaultPreferences,
    }
  }
  return {
    mode,
    provider: 'deepseek',
    credential: '',
    model: '',
    preferences: defaultPreferences,
  }
}

export const AiGenerationScreen = ({
  vault,
  ollamaLifecycle,
}: {
  vault: KeyVault
  ollamaLifecycle?: OllamaLifecycle
}) => {
  const store = useMemo(() => new LocalAiProfileStore(vault), [vault])
  const [profile, setProfile] = useState<LocalAiProfile>(() =>
    store.load() ?? emptyProfile('commercial_api'),
  )
  const [models, setModels] = useState<string[]>([])
  const [status, setStatus] = useState('')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    setModels([])
    setStatus('')
  }, [profile.mode])

  const patchProfile = (patch: Record<string, unknown>) =>
    setProfile((current) => ({ ...current, ...patch }) as LocalAiProfile)

  const discover = async () => {
    setBusy(true)
    setStatus('')
    try {
      const bridge = resolveLocalEdgeInferenceBridge()
      if (!bridge && !browserDirectInferenceAllowed(profile)) {
        throw new Error('Questa modalità richiede lo Sprout Local Edge Runtime.')
      }
      const discovered = bridge
        ? await bridge.discoverModels(profile)
        : await providerForLocalProfile(profile).discoverModels()
      setModels(discovered.map((model) => model.id))
      setStatus(
        discovered.length > 0
          ? `${discovered.length} modelli rilevati direttamente dal dispositivo.`
          : 'Nessun modello disponibile; non è stata applicata alcuna sostituzione.',
      )
    } catch (error) {
      setStatus(error instanceof Error ? error.message : 'Model discovery failed')
    } finally {
      setBusy(false)
    }
  }

  const save = async () => {
    setBusy(true)
    try {
      const result = await store.save(profile)
      setStatus(
        result === 'persisted'
          ? 'Configurazione cifrata nel vault di questo dispositivo.'
          : 'Configurazione valida per questa sessione; abilita il vault persistente per conservarla.',
      )
    } catch (error) {
      setStatus(error instanceof Error ? error.message : 'Unable to save configuration')
    } finally {
      setBusy(false)
    }
  }

  const remove = async () => {
    setBusy(true)
    const result = await store.delete()
    setProfile(emptyProfile('commercial_api'))
    setModels([])
    setStatus(
      result === 'persisted'
        ? 'Configurazione AI eliminata da questo dispositivo.'
        : 'Configurazione AI rimossa dalla sessione corrente.',
    )
    setBusy(false)
  }

  const installOllama = async () => {
    const consented = window.confirm(
      'Installare Ollama dalla distribuzione ufficiale su questo dispositivo? Rimarrà installato e utilizzabile anche da altri progetti.',
    )
    if (!consented) {
      setStatus('Installazione Ollama annullata.')
      return
    }
    if (!ollamaLifecycle) {
      window.open('https://ollama.com/download', '_blank', 'noopener,noreferrer')
      setStatus(
        'Aperta la distribuzione ufficiale Ollama. Dopo l’installazione collega lo Sprout Local Edge Runtime e ripeti il rilevamento.',
      )
      return
    }
    setBusy(true)
    try {
      const installed = await installOllamaWithConsent(ollamaLifecycle, true)
      setStatus(installed ? 'Ollama installato localmente.' : 'Installazione Ollama annullata.')
    } finally {
      setBusy(false)
    }
  }

  const pullOllamaModel = async () => {
    if (!ollamaLifecycle) {
      setStatus('Sprout Local Edge Runtime non disponibile: download modello non avviato.')
      return
    }
    const consented = window.confirm(
      `Scaricare localmente ${OLLAMA_CHECKPOINT_MODEL}? Il modello resterà installato finché non verrà rimosso esplicitamente.`,
    )
    setBusy(true)
    try {
      const pulled = await pullOllamaModelWithSeparateConsent(
        ollamaLifecycle,
        OLLAMA_CHECKPOINT_MODEL,
        consented,
      )
      if (pulled) patchProfile({ model: OLLAMA_CHECKPOINT_MODEL })
      setStatus(pulled ? 'Modello Ollama scaricato localmente.' : 'Download modello annullato.')
    } finally {
      setBusy(false)
    }
  }

  const commercial = profile.mode === 'commercial_api' || profile.mode === 'commercial_privacy'
  const selfHosted = profile.mode === 'lan_inference' || profile.mode === 'private_remote'
  const insecureLanDevelopment =
    profile.mode === 'lan_inference' && profile.baseUrl.startsWith('http://')

  return (
    <div className="settings-content settings-stack ai-generation-settings">
      <section className="settings-section">
        <div className="settings-section-heading">
          <h2 className="settings-section-title">AI / Generazione testo</h2>
          <p className="settings-section-subtitle">{LOCAL_AI_PROFILE_NOTICE}</p>
        </div>
        <div className="settings-group">
          <div className="settings-group-header">
            <h3>Modalità</h3>
            <p>Provider, credenziali e rete restano nel vault locale del device.</p>
          </div>
          <div className="ai-mode-grid">
            {modes.map((mode) => (
              <label key={mode.id} className="ai-mode-option">
                <input
                  type="radio"
                  name="ai-mode"
                  value={mode.id}
                  checked={profile.mode === mode.id}
                  onChange={() => setProfile(emptyProfile(mode.id))}
                />
                <span>{mode.label}</span>
              </label>
            ))}
          </div>
        </div>
      </section>

      <section className="settings-section">
        <div className="settings-group">
          <div className="settings-group-header">
            <h3>Configurazione locale</h3>
          </div>
          <div className="ai-settings-form">
            {insecureLanDevelopment ? (
              <p role="alert" className="settings-warning">
                HTTP è ammesso soltanto per loopback o destinazioni LAN di sviluppo esplicitamente
                abilitate. In produzione LAN è richiesto HTTPS verificato.
              </p>
            ) : null}
            {commercial ? (
              <label>
                Provider
                <select
                  value={profile.provider}
                  onChange={(event) =>
                    patchProfile({ provider: event.target.value as CommercialProvider })
                  }
                >
                  {providers.map((provider) => (
                    <option key={provider.id} value={provider.id}>{provider.label}</option>
                  ))}
                </select>
              </label>
            ) : null}
            {selfHosted ? (
              <label>
                Engine
                <select
                  value={profile.engine}
                  onChange={(event) => patchProfile({ engine: event.target.value as SelfHostedEngine })}
                >
                  <option value="ds4">DS4</option>
                  <option value="ollama">Ollama</option>
                </select>
              </label>
            ) : null}
            {'baseUrl' in profile ? (
              <label>
                Base URL
                <input
                  type="url"
                  value={profile.baseUrl ?? ''}
                  autoComplete="off"
                  onChange={(event) => patchProfile({ baseUrl: event.target.value })}
                />
              </label>
            ) : null}
            {'destination' in profile ? (
              <label>
                Destinazione privata (/32 o /128)
                <input value={profile.destination} onChange={(event) => patchProfile({ destination: event.target.value })} />
              </label>
            ) : null}
            {'credential' in profile ? (
              <label>
                Credential
                <input
                  type="password"
                  value={profile.credential}
                  autoComplete="off"
                  onChange={(event) => patchProfile({ credential: event.target.value })}
                />
              </label>
            ) : null}
            {'token' in profile ? (
              <label>
                Token opzionale
                <input type="password" value={profile.token ?? ''} autoComplete="off" onChange={(event) => patchProfile({ token: event.target.value })} />
              </label>
            ) : null}
            {'tlsPinSha256' in profile ? (
              <label>
                TLS pin SHA-256
                <input type="password" value={profile.tlsPinSha256 ?? ''} autoComplete="off" onChange={(event) => patchProfile({ tlsPinSha256: event.target.value })} />
              </label>
            ) : null}
            <label>
              Modello
              <input
                list="ai-discovered-models"
                value={profile.model}
                autoComplete="off"
                onChange={(event) => patchProfile({ model: event.target.value })}
              />
              <datalist id="ai-discovered-models">
                {models.map((model) => <option key={model} value={model} />)}
              </datalist>
            </label>
            {(profile.mode === 'commercial_api' || profile.mode === 'lan_inference') ? (
              <button type="button" className="secondary-button" disabled={busy} onClick={() => void discover()}>
                Rileva modelli dal dispositivo
              </button>
            ) : null}
            {profile.mode === 'lan_inference' && profile.engine === 'ollama' ? (
              <div className="ai-local-install-actions">
                <button type="button" className="secondary-button" disabled={busy} onClick={() => void installOllama()}>
                  Installa Ollama
                </button>
                <button type="button" className="secondary-button" disabled={busy} onClick={() => void pullOllamaModel()}>
                  Scarica {OLLAMA_CHECKPOINT_MODEL}
                </button>
                <p>Installazione e model pull richiedono consensi separati. Ollama non viene disinstallato dopo i test.</p>
              </div>
            ) : null}
            {profile.mode === 'private_remote' ? (
              <p className="ai-boundary-note" role="status">
                DESIGNED / CONTRACT-TESTED / NOT LIVE-VALIDATED. Nessuna VPN o route viene configurata da Sprout.
              </p>
            ) : null}
            {profile.mode === 'commercial_privacy' ? (
              <div className="ai-boundary-note" role="status">
                <strong>EXPERIMENTAL / NOT YET FORMALLY ENABLED</strong>
                <p>Sprout Local AI Runtime e il modello privacy richiedono due consensi separati. Nessun fallback silenzioso.</p>
                <button type="button" className="secondary-button" disabled>
                  Installa Sprout Local AI Runtime (non disponibile)
                </button>
                <button type="button" className="secondary-button" disabled>
                  Scarica modello privacy (consenso separato)
                </button>
              </div>
            ) : null}
          </div>
        </div>
      </section>

      {status ? <p role="status" className="ai-settings-status">{status}</p> : null}
      <div className="ai-settings-actions">
        <button type="button" className="primary-button" disabled={busy} onClick={() => void save()}>
          Salva su questo dispositivo
        </button>
        <button type="button" className="secondary-button" disabled={busy} onClick={() => void remove()}>
          Elimina configurazione AI da questo dispositivo
        </button>
      </div>
    </div>
  )
}
