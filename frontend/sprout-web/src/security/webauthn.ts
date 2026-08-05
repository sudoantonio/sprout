import type { Uuid } from '../api/contracts'
import { zeroBytes } from './wasm'

interface DescriptorJson {
  type: PublicKeyCredentialType
  id: string
  transports?: AuthenticatorTransport[]
}

interface CreationOptionsJson {
  challenge: string
  rp: PublicKeyCredentialRpEntity
  user: Omit<PublicKeyCredentialUserEntity, 'id'> & { id: string }
  pubKeyCredParams: PublicKeyCredentialParameters[]
  timeout?: number
  attestation?: AttestationConveyancePreference
  authenticatorSelection?: AuthenticatorSelectionCriteria
  excludeCredentials?: DescriptorJson[]
  extensions?: AuthenticationExtensionsClientInputs
}

interface RequestOptionsJson {
  challenge: string
  rpId?: string
  timeout?: number
  userVerification?: UserVerificationRequirement
  allowCredentials?: DescriptorJson[]
  extensions?: AuthenticationExtensionsClientInputs
}

interface PrfExtensionInput extends AuthenticationExtensionsClientInputs {
  prf: {
    eval: {
      first: ArrayBuffer
    }
  }
}

export interface PasskeyCeremonyResult {
  credential: unknown
  credentialId: string
  prfOutput?: Uint8Array
  prfSupported: boolean
}

export interface LocalPrfResult {
  prfOutput?: Uint8Array
  prfSupported: boolean
}

const decodeBase64Url = (value: string): ArrayBuffer => {
  const padding = '='.repeat((4 - (value.length % 4)) % 4)
  const binary = atob(value.replaceAll('-', '+').replaceAll('_', '/') + padding)
  return Uint8Array.from(binary, (character) => character.charCodeAt(0)).buffer
}

const encodeBase64Url = (value: ArrayBuffer): string => {
  let binary = ''
  for (const byte of new Uint8Array(value)) {
    binary += String.fromCharCode(byte)
  }
  return btoa(binary)
    .replaceAll('+', '-')
    .replaceAll('/', '_')
    .replaceAll('=', '')
}

export const isWebAuthnAvailable = (): boolean =>
  typeof PublicKeyCredential !== 'undefined' &&
  typeof navigator !== 'undefined' &&
  Boolean(navigator.credentials)

export const isPlatformAuthenticatorAvailable = async (): Promise<boolean> => {
  if (!isWebAuthnAvailable()) {
    return false
  }
  return PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable()
}

export const isConditionalMediationAvailable = async (): Promise<boolean> => {
  if (!isWebAuthnAvailable()) {
    return false
  }
  const credentialType = PublicKeyCredential as typeof PublicKeyCredential & {
    isConditionalMediationAvailable?: () => Promise<boolean>
  }
  return credentialType.isConditionalMediationAvailable?.() ?? false
}

const publicKeyOptions = <T>(options: unknown): T => {
  if (
    typeof options === 'object' &&
    options !== null &&
    'publicKey' in options
  ) {
    return (options as { publicKey: T }).publicKey
  }
  return options as T
}

export const prfInputForDevice = async (deviceId: Uuid): Promise<Uint8Array> =>
  new Uint8Array(
    await crypto.subtle.digest(
      'SHA-256',
      new TextEncoder().encode(`sprout-webauthn-prf-v1:${deviceId}`),
    ),
  )

const serializeCredential = (credential: PublicKeyCredential): unknown => {
  const toJSON = (
    credential as unknown as {
      toJSON?: () => unknown
    }
  ).toJSON
  if (toJSON) {
    return toJSON.call(credential)
  }

  if ('attestationObject' in credential.response) {
    const response =
      credential.response as AuthenticatorAttestationResponse
    return {
      id: credential.id,
      rawId: encodeBase64Url(credential.rawId),
      type: 'public-key',
      extensions: credential.getClientExtensionResults(),
      response: {
        clientDataJSON: encodeBase64Url(
          response.clientDataJSON,
        ),
        attestationObject: encodeBase64Url(
          response.attestationObject,
        ),
        transports: response.getTransports?.() ?? [],
      },
    }
  }

  const response = credential.response as AuthenticatorAssertionResponse
  return {
    id: credential.id,
    rawId: encodeBase64Url(credential.rawId),
    type: 'public-key',
    extensions: credential.getClientExtensionResults(),
    response: {
      clientDataJSON: encodeBase64Url(response.clientDataJSON),
      authenticatorData: encodeBase64Url(response.authenticatorData),
      signature: encodeBase64Url(response.signature),
      userHandle: response.userHandle
        ? encodeBase64Url(response.userHandle)
        : null,
    },
  }
}

const extractPrf = (
  credential: PublicKeyCredential,
): Pick<PasskeyCeremonyResult, 'prfOutput' | 'prfSupported'> => {
  const output = credential.getClientExtensionResults()
  const first = output.prf?.results?.first
  const copied = first
    ? new Uint8Array(
        first instanceof ArrayBuffer
          ? first.slice(0)
          : first.buffer.slice(
              first.byteOffset,
              first.byteOffset + first.byteLength,
            ),
      )
    : undefined
  return {
    prfOutput: copied,
    prfSupported: Boolean(output.prf?.enabled || first),
  }
}

const prfExtensions = (
  existing: AuthenticationExtensionsClientInputs | undefined,
  input: Uint8Array,
): AuthenticationExtensionsClientInputs =>
  ({
    ...existing,
    prf: { eval: { first: input.buffer } },
  }) as PrfExtensionInput

export const createPasskey = async (
  serverOptions: unknown,
  deviceId: Uuid,
): Promise<PasskeyCeremonyResult> => {
  if (!isWebAuthnAvailable()) {
    throw new Error('Passkeys are not supported by this browser')
  }

  const options = publicKeyOptions<CreationOptionsJson>(serverOptions)
  const prfInput = await prfInputForDevice(deviceId)
  try {
    const credential = await navigator.credentials.create({
      publicKey: {
        ...options,
        challenge: decodeBase64Url(options.challenge),
        user: { ...options.user, id: decodeBase64Url(options.user.id) },
        excludeCredentials: options.excludeCredentials?.map((descriptor) => ({
          ...descriptor,
          id: decodeBase64Url(descriptor.id),
        })),
        extensions: prfExtensions(options.extensions, prfInput),
      },
    })

    if (!(credential instanceof PublicKeyCredential)) {
      throw new Error('The browser did not return a passkey credential')
    }

    return {
      credential: serializeCredential(credential),
      credentialId: credential.id,
      ...extractPrf(credential),
    }
  } finally {
    zeroBytes(prfInput)
  }
}

export const getPasskey = async (
  serverOptions: unknown,
  deviceId: Uuid,
): Promise<PasskeyCeremonyResult> => {
  if (!isWebAuthnAvailable()) {
    throw new Error('Passkeys are not supported by this browser')
  }

  const options = publicKeyOptions<RequestOptionsJson>(serverOptions)
  const prfInput = await prfInputForDevice(deviceId)
  try {
    const credential = await navigator.credentials.get({
      publicKey: {
        ...options,
        challenge: decodeBase64Url(options.challenge),
        allowCredentials: options.allowCredentials?.map((descriptor) => ({
          ...descriptor,
          id: decodeBase64Url(descriptor.id),
        })),
        extensions: prfExtensions(options.extensions, prfInput),
      },
    })

    if (!(credential instanceof PublicKeyCredential)) {
      throw new Error('The browser did not return a passkey credential')
    }

    return {
      credential: serializeCredential(credential),
      credentialId: credential.id,
      ...extractPrf(credential),
    }
  } finally {
    zeroBytes(prfInput)
  }
}

export const getLocalVaultPrf = async (
  credentialId: string,
  deviceId: Uuid,
): Promise<LocalPrfResult> => {
  if (!isWebAuthnAvailable()) {
    throw new Error('Passkeys are not supported by this browser')
  }
  const challenge = crypto.getRandomValues(new Uint8Array(32))
  const prfInput = await prfInputForDevice(deviceId)
  try {
    const credential = await navigator.credentials.get({
      publicKey: {
        challenge,
        allowCredentials: [
          {
            type: 'public-key',
            id: decodeBase64Url(credentialId),
          },
        ],
        userVerification: 'required',
        extensions: prfExtensions(undefined, prfInput),
      },
    })
    if (!(credential instanceof PublicKeyCredential)) {
      throw new Error('The browser did not return a passkey credential')
    }
    if (credential.id !== credentialId) {
      throw new Error('The passkey does not match the local encrypted vault')
    }
    return extractPrf(credential)
  } finally {
    zeroBytes(challenge, prfInput)
  }
}
