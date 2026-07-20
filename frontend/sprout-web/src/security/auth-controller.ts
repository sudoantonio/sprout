import { ApiClient } from '../api/client'
import type {
  DeviceKeyPackageView,
  SessionResponse,
  Uuid,
} from '../api/contracts'
import { EncryptedDatabase } from '../storage/encrypted-db'
import {
  createPasskey,
  getLocalVaultPrf,
  getPasskey,
} from './webauthn'
import { KeyVault } from './key-vault'
import {
  bytesToBase64,
  encryptDocument,
  generateDeviceSecrets,
  zeroBytes,
} from './wasm'

export interface AuthenticationOutcome {
  session: SessionResponse
  requiresAuthorizedDevice: boolean
  prfSupported: boolean
}

export interface LocalUnlockOutcome {
  deviceId: Uuid
  identityId?: Uuid
}

export const shouldProvisionDevice = (
  packages: Pick<DeviceKeyPackageView, 'revoked_at'>[],
): boolean => !packages.some((item) => item.revoked_at === null)

const encryptedBootstrapPayload = async (
  deviceId: Uuid,
  content: Record<string, unknown>,
): Promise<string> => {
  const key = crypto.getRandomValues(new Uint8Array(32))
  try {
    const encrypted = await encryptDocument(content, {
      projectId: deviceId,
      resourceId: deviceId,
      keyId: crypto.randomUUID(),
      kind: 'project',
      aggregateVersion: 0,
      keyEpoch: 1,
      resourceKey: key,
    })
    return bytesToBase64(
      new TextEncoder().encode(JSON.stringify(encrypted)),
    )
  } finally {
    zeroBytes(key)
  }
}

export class AuthController {
  readonly #api: ApiClient
  readonly #database: EncryptedDatabase
  readonly vault: KeyVault

  constructor(api: ApiClient, database: EncryptedDatabase) {
    this.#api = api
    this.#database = database
    this.vault = new KeyVault(database)
  }

  async hasLocalVault(deviceId: Uuid): Promise<boolean> {
    const record = await this.#database.getVault(deviceId)
    return Boolean(record?.credentialId)
  }

  async unlockLocalVault(deviceId: Uuid): Promise<LocalUnlockOutcome> {
    const record = await this.#database.getVault(deviceId)
    if (!record?.credentialId) {
      throw new Error('No passkey-wrapped local vault is available')
    }
    const local = await getLocalVaultPrf(record.credentialId, deviceId)
    if (!local.prfOutput) {
      throw new Error(
        'This authenticator did not return PRF output; use another authorized device or recovery',
      )
    }
    try {
      const unlocked = await this.vault.unlockWithPrf(
        deviceId,
        local.prfOutput,
      )
      if (!unlocked) {
        throw new Error('The local encrypted vault could not be unlocked')
      }
      return {
        deviceId,
        identityId: this.vault.localIdentityId,
      }
    } catch {
      throw new Error('The passkey could not unlock this local encrypted vault')
    } finally {
      zeroBytes(local.prfOutput)
    }
  }

  startSignup(input: {
    email: string
    identityHandle: string
    deviceId: Uuid
  }): Promise<{ accepted: boolean }> {
    return encryptedBootstrapPayload(input.deviceId, { schema: 1 }).then(
      (encryptedProfile) =>
        this.#api.startEmailVerification({
          email: input.email,
          identity_handle: input.identityHandle,
          encrypted_profile_b64: encryptedProfile,
        }),
    )
  }

  async finishSignup(input: {
    identityId: Uuid
    token: string
    deviceId: Uuid
  }): Promise<AuthenticationOutcome> {
    const session = await this.#finishEmail(
      'verification',
      input.identityId,
      input.token,
      input.deviceId,
    )
    const provisioned = await this.#provisionDevice(
      session.device_id,
      session.identity_id,
    )
    return {
      session,
      requiresAuthorizedDevice: !provisioned,
      prfSupported: false,
    }
  }

  startRecovery(email: string): Promise<{ accepted: boolean }> {
    return this.#api.startEmailRecovery(email)
  }

  async finishRecovery(input: {
    identityId: Uuid
    token: string
    deviceId: Uuid
  }): Promise<AuthenticationOutcome> {
    const session = await this.#finishEmail(
      'recovery',
      input.identityId,
      input.token,
      input.deviceId,
    )
    await this.#provisionDevice(session.device_id, session.identity_id)
    return {
      session,
      requiresAuthorizedDevice: true,
      prfSupported: false,
    }
  }

  async authenticatePasskey(input: {
    identityId: Uuid
    identityHandle: string
    deviceId: Uuid
  }): Promise<AuthenticationOutcome> {
    const challenge = await this.#api.startPasskeyAuthentication({
      identity_id: input.identityId,
      identity_handle: input.identityHandle,
    })
    const passkey = await getPasskey(challenge.options, input.deviceId)
    const encryptedLabel = await encryptedBootstrapPayload(input.deviceId, {
      schema: 1,
      kind: 'web',
    })
    const session = await this.#api.finishPasskeyAuthentication({
      identity_id: input.identityId,
      challenge_id: challenge.challenge_id,
      credential: passkey.credential,
      device_id: input.deviceId,
      device_kind: 'web',
      encrypted_device_label_b64: encryptedLabel,
    })
    this.#api.setSession(session.token)

    let unlocked = false
    const persistenceOutput = passkey.prfOutput?.slice()
    if (passkey.prfOutput) {
      unlocked = await this.vault.unlockWithPrf(
        session.device_id,
        passkey.prfOutput,
      )
    }
    if (!unlocked) {
      const provisioned = await this.#provisionDevice(
        session.device_id,
        session.identity_id,
      )
      if (persistenceOutput && provisioned) {
        await this.vault.enablePrfPersistence(
          persistenceOutput,
          passkey.credentialId,
        )
      } else {
        zeroBytes(persistenceOutput)
      }
    } else {
      zeroBytes(persistenceOutput)
    }

    return {
      session,
      requiresAuthorizedDevice: !unlocked,
      prfSupported: passkey.prfSupported,
    }
  }

  async registerPasskey(deviceId: Uuid): Promise<{
    passkeyId: Uuid
    prfSupported: boolean
  }> {
    const challenge = await this.#api.startPasskeyRegistration()
    const passkey = await createPasskey(challenge.options, deviceId)
    const result = await this.#api.finishPasskeyRegistration(
      challenge.challenge_id,
      passkey.credential,
    )
    if (passkey.prfOutput) {
      await this.vault.enablePrfPersistence(
        passkey.prfOutput,
        passkey.credentialId,
      )
    }
    return {
      passkeyId: result.passkey_id,
      prfSupported: passkey.prfSupported,
    }
  }

  logout(): void {
    this.vault.clearMemory()
    this.#api.setSession()
  }

  async #finishEmail(
    kind: 'verification' | 'recovery',
    identityId: Uuid,
    token: string,
    deviceId: Uuid,
  ): Promise<SessionResponse> {
    const encryptedLabel = await encryptedBootstrapPayload(deviceId, {
      schema: 1,
      kind: 'web',
    })
    const request = {
      identity_id: identityId,
      token,
      device_id: deviceId,
      device_kind: 'web' as const,
      encrypted_device_label_b64: encryptedLabel,
    }
    const session =
      kind === 'verification'
        ? await this.#api.finishEmailVerification(request)
        : await this.#api.finishEmailRecovery(request)
    this.#api.setSession(session.token)
    return session
  }

  async #provisionDevice(
    deviceId: Uuid,
    identityId: Uuid,
  ): Promise<boolean> {
    const packages = await this.#api.listDevicePackages(deviceId)
    if (!shouldProvisionDevice(packages)) {
      return false
    }
    const secrets = await generateDeviceSecrets(deviceId)
    this.vault.setSessionSecrets(deviceId, secrets, identityId)
    const registered = await this.#api.registerDevicePackage(
      deviceId,
      bytesToBase64(secrets.publicPackage),
    )
    secrets.keyVersion = registered.key_version
    return true
  }
}
