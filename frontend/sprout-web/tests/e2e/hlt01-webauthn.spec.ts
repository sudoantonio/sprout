import { existsSync } from 'node:fs'
import { readFile } from 'node:fs/promises'
import { expect, test } from '@playwright/test'

interface IdentityEvidence {
  alice_identity_id: string
  alice_identity_handle: string
  alice_session: string
}

const evidencePath =
  process.env.HLT01_EVIDENCE_PATH ??
  process.env.HLT05_EVIDENCE_PATH ??
  '/evidence/hlt05.json'
const evidencePathWasConfigured = Boolean(
  process.env.HLT01_EVIDENCE_PATH ?? process.env.HLT05_EVIDENCE_PATH,
)

test('HLT-01 and T-LLR-01.2 enforce a single-use, origin-bound, UV passkey ceremony', async ({
  context,
  page,
  browserName,
}) => {
  test.skip(browserName !== 'chromium', 'Chromium exposes the virtual authenticator protocol')
  test.skip(
    !evidencePathWasConfigured && !existsSync(evidencePath),
    'Requires backend-generated HLT-05 identity evidence',
  )
  const evidence = JSON.parse(
    await readFile(evidencePath, 'utf8'),
  ) as IdentityEvidence
  const cdp = await context.newCDPSession(page)
  await cdp.send('WebAuthn.enable')
  const { authenticatorId } = await cdp.send(
    'WebAuthn.addVirtualAuthenticator',
    {
      options: {
        protocol: 'ctap2',
        ctap2Version: 'ctap2_1',
        transport: 'internal',
        hasResidentKey: true,
        hasUserVerification: true,
        isUserVerified: true,
        automaticPresenceSimulation: true,
      },
    },
  )

  try {
    await page.goto('/')
    const registration = await page.evaluate(async (identity) => {
      const { ApiClient } = await import('/src/api/client.ts')
      const { createPasskey } = await import('/src/security/webauthn.ts')
      const api = new ApiClient()
      api.setSession(identity.alice_session)
      const challenge = await api.startPasskeyRegistration()
      const passkey = await createPasskey(
        challenge.options,
        crypto.randomUUID(),
      )
      return api.finishPasskeyRegistration(
        challenge.challenge_id,
        passkey.credential,
      )
    }, evidence)
    expect(registration.passkey_id).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i,
    )

    const validChallenge = await page.evaluate(async (identity) => {
      const { ApiClient } = await import('/src/api/client.ts')
      return new ApiClient().startPasskeyAuthentication({
        identity_id: identity.alice_identity_id,
        identity_handle: identity.alice_identity_handle,
      })
    }, evidence)
    const validDeviceId = crypto.randomUUID()
    const validAssertion = await page.evaluate(
      async ({ options, deviceId }) => {
        const { getPasskey } = await import('/src/security/webauthn.ts')
        return getPasskey(options, deviceId)
      },
      { options: validChallenge.options, deviceId: validDeviceId },
    )
    const validInput = {
      identity_id: evidence.alice_identity_id,
      challenge_id: validChallenge.challenge_id,
      credential: validAssertion.credential,
      device_id: validDeviceId,
      device_kind: 'web' as const,
      encrypted_device_label_b64: btoa('hlt01-passkey-device'),
    }
    const validSession = await page.evaluate(async (input) => {
      const { ApiClient } = await import('/src/api/client.ts')
      return new ApiClient().finishPasskeyAuthentication(input)
    }, validInput)
    expect(validSession.token).toMatch(/^v1\./)

    const replayStatus = await page.evaluate(async (input) => {
      const { ApiClient, ApiError } = await import('/src/api/client.ts')
      try {
        await new ApiClient().finishPasskeyAuthentication(input)
        return 200
      } catch (error) {
        return error instanceof ApiError ? error.status : 0
      }
    }, validInput)
    expect(replayStatus).toBeGreaterThanOrEqual(400)

    const wrongRpChallenge = await page.evaluate(async (identity) => {
      const { ApiClient } = await import('/src/api/client.ts')
      return new ApiClient().startPasskeyAuthentication({
        identity_id: identity.alice_identity_id,
        identity_handle: identity.alice_identity_handle,
      })
    }, evidence)
    const wrongRpError = await page.evaluate(async (options) => {
      const { getPasskey } = await import('/src/security/webauthn.ts')
      const mutated = structuredClone(options) as {
        publicKey?: { rpId?: string }
        rpId?: string
      }
      const publicKey = mutated.publicKey ?? mutated
      publicKey.rpId = 'wrong.invalid'
      try {
        await getPasskey(mutated, crypto.randomUUID())
        return 'accepted'
      } catch (error) {
        return error instanceof DOMException ? error.name : 'error'
      }
    }, wrongRpChallenge.options)
    expect(['SecurityError', 'NotAllowedError']).toContain(wrongRpError)

    const wrongOriginChallenge = await page.evaluate(async (identity) => {
      const { ApiClient } = await import('/src/api/client.ts')
      return new ApiClient().startPasskeyAuthentication({
        identity_id: identity.alice_identity_id,
        identity_handle: identity.alice_identity_handle,
      })
    }, evidence)
    await page.goto('http://127.0.0.1:4173/')
    const wrongOriginError = await page.evaluate(async (options) => {
      const { getPasskey } = await import('/src/security/webauthn.ts')
      try {
        await getPasskey(options, crypto.randomUUID())
        return 'accepted'
      } catch (error) {
        return error instanceof DOMException ? error.name : 'error'
      }
    }, wrongOriginChallenge.options)
    expect(['SecurityError', 'NotAllowedError']).toContain(wrongOriginError)

    await page.goto('http://localhost:4173/')
    const missingUvChallenge = await page.evaluate(async (identity) => {
      const { ApiClient } = await import('/src/api/client.ts')
      return new ApiClient().startPasskeyAuthentication({
        identity_id: identity.alice_identity_id,
        identity_handle: identity.alice_identity_handle,
      })
    }, evidence)
    await cdp.send('WebAuthn.setUserVerified', {
      authenticatorId,
      isUserVerified: false,
    })
    const missingUv = await page.evaluate(
      async ({ identity, challenge }) => {
        const { ApiClient, ApiError } = await import('/src/api/client.ts')
        const { getPasskey } = await import('/src/security/webauthn.ts')
        const mutated = structuredClone(challenge.options) as {
          publicKey?: { userVerification?: string }
          userVerification?: string
        }
        const publicKey = mutated.publicKey ?? mutated
        publicKey.userVerification = 'discouraged'
        try {
          const assertion = await getPasskey(
            mutated,
            crypto.randomUUID(),
          )
          await new ApiClient().finishPasskeyAuthentication({
            identity_id: identity.alice_identity_id,
            challenge_id: challenge.challenge_id,
            credential: assertion.credential,
            device_id: crypto.randomUUID(),
            device_kind: 'web',
            encrypted_device_label_b64: btoa('hlt01-no-uv-device'),
          })
          return 'accepted'
        } catch (error) {
          if (error instanceof ApiError) {
            return `api:${error.status}`
          }
          return error instanceof DOMException
            ? `browser:${error.name}`
            : 'error'
        }
      },
      { identity: evidence, challenge: missingUvChallenge },
    )
    expect(missingUv).toMatch(/^(api:401|browser:NotAllowedError)$/)
  } finally {
    await cdp.send('WebAuthn.removeVirtualAuthenticator', {
      authenticatorId,
    })
    await cdp.send('WebAuthn.disable')
  }
})
