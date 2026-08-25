import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
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
})
