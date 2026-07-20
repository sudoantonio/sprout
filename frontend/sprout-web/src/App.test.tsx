import { render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiClient } from './api/client'
import App from './App'
import { EncryptedDatabase } from './storage/encrypted-db'

afterEach(() => {
  vi.restoreAllMocks()
  localStorage.clear()
})

describe('Sprout API shell', () => {
  it('starts with real account ceremonies and the passkey limitation', () => {
    render(<App />)
    expect(
      screen.getByRole('heading', {
        name: /your work stays readable only on authorized devices/i,
      }),
    ).toBeInTheDocument()
    expect(
      screen.getByRole('heading', { name: /sign in with a passkey/i }),
    ).toBeInTheDocument()
    expect(
      screen.getByText(/a passkey does not reveal encryption keys/i),
    ).toBeInTheDocument()
  })

  it('renders the authenticated API empty state without default projects', async () => {
    const database = {
      close: vi.fn(),
      getVault: vi.fn().mockResolvedValue(undefined),
      queueCount: vi.fn().mockResolvedValue(0),
      listConflicts: vi.fn().mockResolvedValue([]),
      putRecord: vi.fn().mockResolvedValue(undefined),
    } as unknown as EncryptedDatabase
    vi.spyOn(EncryptedDatabase, 'open').mockResolvedValue(database)

    const api = {
      setSession: vi.fn(),
      listProjects: vi.fn().mockResolvedValue([]),
      getRetentionPreference: vi.fn().mockResolvedValue({
        preference: {
          auto_export_enabled: false,
          updated_at: new Date().toISOString(),
        },
      }),
      listRetentionArchives: vi.fn().mockResolvedValue({ archives: [] }),
      listRetentionWarnings: vi.fn().mockResolvedValue({ warnings: [] }),
    } as unknown as ApiClient
    render(
      <App
        apiClient={api}
        initialSession={{
          token: crypto.randomUUID(),
          expires_at: new Date(Date.now() + 60_000).toISOString(),
          identity_id: crypto.randomUUID(),
          device_id: crypto.randomUUID(),
        }}
      />,
    )

    await waitFor(() =>
      expect(api.listProjects).toHaveBeenCalledTimes(1),
    )
    expect(
      screen.getByRole('heading', { name: 'No project selected' }),
    ).toBeInTheDocument()
    expect(
      screen.queryByText(/home|studio launch|demo ciphertext/i),
    ).not.toBeInTheDocument()
  })

  it('makes offline account ceremony state explicit', async () => {
    vi.spyOn(navigator, 'onLine', 'get').mockReturnValue(false)
    render(<App />)
    expect(
      screen.getByText(/account ceremonies require a network connection/i),
    ).toBeInTheDocument()
    await waitFor(() =>
      expect(
        screen.getByRole('button', { name: /working|use passkey/i }),
      ).toBeDisabled(),
    )
  })
})
