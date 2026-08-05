import { describe, expect, it } from 'vitest'
import { ApiError } from '../api/client'
import { authErrorMessage } from './auth-errors'

describe('authErrorMessage', () => {
  it('explains passkey sign-in before verification', () => {
    expect(
      authErrorMessage(new ApiError(401, 'unauthorized: authentication required'), 'signin'),
    ).toMatch(/completa prima Crea → Verifica/i)
  })

  it('explains invalid verification tokens', () => {
    expect(
      authErrorMessage(
        new ApiError(400, 'verification token is invalid or expired'),
        'verify',
      ),
    ).toMatch(/Token non valido/i)
  })

  it('explains already verified accounts', () => {
    expect(
      authErrorMessage(
        new ApiError(
          400,
          'account is already verified; use passkey sign-in or account recovery',
        ),
        'verify',
      ),
    ).toMatch(/già verificato/i)
  })
})
