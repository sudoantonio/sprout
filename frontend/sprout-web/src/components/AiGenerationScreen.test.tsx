import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { KeyVault } from '../security/key-vault'
import { AiGenerationScreen } from './AiGenerationScreen'

const localSettings = new Map<string, string>()
const vault = {
  getLocalSetting: (key: string) => localSettings.get(key),
  putLocalSetting: async (key: string, value: string) => {
    localSettings.set(key, value)
    return false
  },
  deleteLocalSetting: async (key: string) => {
    localSettings.delete(key)
    return false
  },
} as unknown as KeyVault

describe('AI generation local profile screen', () => {
  beforeEach(() => localSettings.clear())
  afterEach(() => vi.unstubAllGlobals())

  it('shows exactly four modes and the device-only notice', async () => {
    const user = userEvent.setup()
    render(<AiGenerationScreen vault={vault} />)
    expect(screen.getAllByRole('radio')).toHaveLength(4)
    expect(
      screen.getByText('Valida soltanto su questo dispositivo — non sincronizzata con Sprout'),
    ).toBeInTheDocument()
    await user.click(screen.getByLabelText('D. API commerciale con protezione privacy locale'))
    expect(screen.getByText('EXPERIMENTAL / NOT YET FORMALLY ENABLED')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Installa Sprout Local AI Runtime/ })).toBeDisabled()
  })

  it('offers explicit local deletion', () => {
    render(<AiGenerationScreen vault={vault} />)
    expect(
      screen.getByRole('button', { name: 'Elimina configurazione AI da questo dispositivo' }),
    ).toBeInTheDocument()
  })

  it('discovers models directly from the configured device provider', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({
      data: [{ id: 'deepseek-v4-flash' }, { id: 'deepseek-v4-pro' }],
    }), { status: 200, headers: { 'Content-Type': 'application/json' } })))
    const user = userEvent.setup()
    render(<AiGenerationScreen vault={vault} />)

    await user.click(screen.getByRole('button', { name: 'Rileva modelli dal dispositivo' }))

    expect(await screen.findByText('2 modelli rilevati direttamente dal dispositivo.')).toBeVisible()
    expect(document.querySelector('datalist option[value="deepseek-v4-flash"]')).toBeInTheDocument()
    expect(fetch).toHaveBeenCalledWith(
      'https://api.deepseek.com/v1/models',
      expect.objectContaining({ cache: 'no-store', credentials: 'omit' }),
    )
  })
})
