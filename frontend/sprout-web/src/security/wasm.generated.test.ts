/// <reference types="node" />

// @vitest-environment node

import { readFile } from 'node:fs/promises'
import { fileURLToPath, pathToFileURL } from 'node:url'
import path from 'node:path'
import { describe, expect, it } from 'vitest'
import {
  combineRecoverySecret,
  configureCryptoModuleForTests,
  decryptDocument,
  encryptDocument,
  signDual as signDualForPwa,
  splitRecoverySecret,
  unwrapResourceKeyForRecipient,
  wrapResourceKeyForRecipient,
} from './wasm'
import type { GeneratedSproutCryptoModule } from './wasm'

const encoder = new TextEncoder()
const bytes = (start: number): Uint8Array =>
  Uint8Array.from({ length: 16 }, (_, index) => start + index)
const hex = (value: Uint8Array): string =>
  Array.from(value, (byte) => byte.toString(16).padStart(2, '0')).join('')
const fromHex = (value: string): Uint8Array =>
  Uint8Array.from(
    value.match(/../g)?.map((byte) => Number.parseInt(byte, 16)) ?? [],
  )

interface CryptoVectorCorpus {
  corpus_version: number
  protocol_version: number
  audit_status: string
  aes_gcm_aad: Record<
    | 'canonical_header'
    | 'context'
    | 'dek'
    | 'encrypted_payload'
    | 'plaintext'
    | 'wrong_context',
    string
  >
  dual_signature: Record<
    | 'context'
    | 'ed25519_public_key'
    | 'ed25519_signature'
    | 'message'
    | 'ml_dsa_65_public_key'
    | 'ml_dsa_65_signature'
    | 'wrong_context',
    string
  >
  resource_envelope: {
    audit_status: string
    context: string
    envelope: string
    previous_epoch_hash: string
    recipient_device_id: string
    recipient_ml_kem_768_private_key: string
    recipient_x25519_private_key: string
    resource_epoch: number
    resource_id: string
    resource_key: string
    suite_version: number
    wrong_context: string
  }
  recovery_n_of_n: {
    bundle: string
    context: string
    participant_count: number
    secret: string
    shares: string[]
    wrong_context: string
  }
}

const loadVectorCorpus = async (): Promise<CryptoVectorCorpus> => {
  const corpusPath = fileURLToPath(
    new URL('../../../../tests/vectors/crypto-v1.json', import.meta.url),
  )
  return JSON.parse(
    await readFile(corpusPath, 'utf8'),
  ) as CryptoVectorCorpus
}

const loadGeneratedModule =
  async (): Promise<GeneratedSproutCryptoModule> => {
    const webRoot = fileURLToPath(new URL('../../', import.meta.url))
    const generatedModulePath = path.join(
      webRoot,
      'public/wasm/sprout_crypto.js',
    )
    const wasmPath = path.join(
      webRoot,
      'public/wasm/sprout_crypto_bg.wasm',
    )
    const [module, wasm] = await Promise.all([
      import(
        /* @vite-ignore */ pathToFileURL(generatedModulePath).href
      ) as Promise<GeneratedSproutCryptoModule>,
      readFile(wasmPath),
    ])
    await module.default?.({ module_or_path: wasm })
    module.initialize()
    return module
  }

describe('generated Rust/WASM crypto boundary', () => {
  it('consumes the frozen native corpus byte-for-byte', async () => {
    const [module, vectors] = await Promise.all([
      loadGeneratedModule(),
      loadVectorCorpus(),
    ])
    expect(vectors.corpus_version).toBe(1)
    expect(vectors.protocol_version).toBe(1)
    expect(vectors.audit_status).toBe('production_audit_required')

    const encodedHeader = fromHex(
      vectors.aes_gcm_aad.canonical_header,
    )
    const generatedHeader = module.canonicalHeader(
      1,
      1,
      1,
      encodedHeader.slice(11, 27),
      encodedHeader.slice(27, 43),
      7n,
      encodedHeader.slice(51, 83),
      fromHex(vectors.aes_gcm_aad.context),
    )
    expect(generatedHeader).toEqual(encodedHeader)
    const encryptedPayload = fromHex(
      vectors.aes_gcm_aad.encrypted_payload,
    )
    expect(
      module.decrypt(
        fromHex(vectors.aes_gcm_aad.dek),
        encryptedPayload,
        generatedHeader,
      ),
    ).toEqual(fromHex(vectors.aes_gcm_aad.plaintext))
    const tamperedPayload = encryptedPayload.slice()
    tamperedPayload[tamperedPayload.length - 1] ^= 1
    expect(() =>
      module.decrypt(
        fromHex(vectors.aes_gcm_aad.dek),
        tamperedPayload,
        generatedHeader,
      ),
    ).toThrow()
    const wrongHeader = module.canonicalHeader(
      1,
      1,
      1,
      encodedHeader.slice(11, 27),
      encodedHeader.slice(27, 43),
      7n,
      encodedHeader.slice(51, 83),
      fromHex(vectors.aes_gcm_aad.wrong_context),
    )
    expect(() =>
      module.decrypt(
        fromHex(vectors.aes_gcm_aad.dek),
        encryptedPayload,
        wrongHeader,
      ),
    ).toThrow()

    const dual = vectors.dual_signature
    const dualArguments = [
      fromHex(dual.ed25519_public_key),
      fromHex(dual.ed25519_signature),
      fromHex(dual.ml_dsa_65_public_key),
      fromHex(dual.ml_dsa_65_signature),
      fromHex(dual.message),
      fromHex(dual.context),
    ] as const
    expect(module.verifyDual(...dualArguments)).toBe(true)
    expect(
      module.verifyDual(
        dualArguments[0],
        dualArguments[1],
        dualArguments[2],
        dualArguments[3],
        dualArguments[4],
        fromHex(dual.wrong_context),
      ),
    ).toBe(false)
    const tamperedEd25519 = dualArguments[1].slice()
    tamperedEd25519[0] ^= 1
    expect(
      module.verifyDual(
        dualArguments[0],
        tamperedEd25519,
        dualArguments[2],
        dualArguments[3],
        dualArguments[4],
        dualArguments[5],
      ),
    ).toBe(false)
    const tamperedMlDsa = dualArguments[3].slice()
    tamperedMlDsa[0] ^= 1
    expect(
      module.verifyDual(
        dualArguments[0],
        dualArguments[1],
        dualArguments[2],
        tamperedMlDsa,
        dualArguments[4],
        dualArguments[5],
      ),
    ).toBe(false)

    const wrapped = vectors.resource_envelope
    expect(wrapped.suite_version).toBe(0x8001)
    expect(wrapped.audit_status).toBe('production_audit_required')
    const envelope = fromHex(wrapped.envelope)
    const unwrapped = module.unwrapResourceKey(
      envelope,
      fromHex(wrapped.recipient_x25519_private_key),
      fromHex(wrapped.recipient_ml_kem_768_private_key),
      fromHex(wrapped.resource_id),
      fromHex(wrapped.recipient_device_id),
      BigInt(wrapped.resource_epoch),
      fromHex(wrapped.previous_epoch_hash),
      fromHex(wrapped.context),
    )
    expect(unwrapped.auditStatus).toBe('production_audit_required')
    expect(unwrapped.resourceKey).toEqual(fromHex(wrapped.resource_key))
    expect(() =>
      module.unwrapResourceKey(
        envelope,
        fromHex(wrapped.recipient_x25519_private_key),
        fromHex(wrapped.recipient_ml_kem_768_private_key),
        fromHex(wrapped.resource_id),
        fromHex(wrapped.recipient_device_id),
        BigInt(wrapped.resource_epoch),
        fromHex(wrapped.previous_epoch_hash),
        fromHex(wrapped.wrong_context),
      ),
    ).toThrow()
    const tamperedEnvelope = envelope.slice()
    tamperedEnvelope[tamperedEnvelope.length - 1] ^= 1
    expect(() =>
      module.unwrapResourceKey(
        tamperedEnvelope,
        fromHex(wrapped.recipient_x25519_private_key),
        fromHex(wrapped.recipient_ml_kem_768_private_key),
        fromHex(wrapped.resource_id),
        fromHex(wrapped.recipient_device_id),
        BigInt(wrapped.resource_epoch),
        fromHex(wrapped.previous_epoch_hash),
        fromHex(wrapped.context),
      ),
    ).toThrow()

    const recovery = vectors.recovery_n_of_n
    const shareSet = new module.RecoveryShareSet()
    for (const share of recovery.shares) {
      shareSet.addShare(fromHex(share))
    }
    expect(shareSet.bundle).toEqual(fromHex(recovery.bundle))
    const recovered = module.combineRecoverySecretNOfN(
      shareSet,
      fromHex(recovery.context),
    )
    expect(recovered.secret).toEqual(fromHex(recovery.secret))
    const recoveredBundle = module.combineRecoverySecretBundleNOfN(
      fromHex(recovery.bundle),
      fromHex(recovery.context),
    )
    expect(recoveredBundle.secret).toEqual(fromHex(recovery.secret))
    const incomplete = new module.RecoveryShareSet()
    for (const share of recovery.shares.slice(0, -1)) {
      incomplete.addShare(fromHex(share))
    }
    expect(() =>
      module.combineRecoverySecretNOfN(
        incomplete,
        fromHex(recovery.context),
      ),
    ).toThrow()
    expect(() =>
      module.combineRecoverySecretNOfN(
        shareSet,
        fromHex(recovery.wrong_context),
      ),
    ).toThrow()
    const tamperedShare = fromHex(recovery.shares[0])
    tamperedShare[tamperedShare.length - 1] ^= 1
    expect(() => {
      const invalid = new module.RecoveryShareSet()
      invalid.addShare(tamperedShare)
    }).toThrow()

    recoveredBundle.destroy()
    recovered.destroy()
    incomplete.destroy()
    shareSet.destroy()
    unwrapped.destroy()
  }, 30_000)

  it('loads the web build and matches native vectors and contracts', async () => {
    const module = await loadGeneratedModule()
    const requiredExports: Array<keyof GeneratedSproutCryptoModule> = [
      'initialize',
      'hash',
      'canonicalHeader',
      'encrypt',
      'decrypt',
      'generateDevicePackage',
      'signDual',
      'verifyDual',
      'wrapResourceKey',
      'unwrapResourceKey',
      'splitRecoverySecretNOfN',
      'combineRecoverySecretNOfN',
      'combineRecoverySecretBundleNOfN',
      'RecoveryShareSet',
    ]
    for (const name of requiredExports) {
      expect(typeof module[name], name).toBe('function')
    }

    const vectorInput = encoder.encode('sprout-wasm-parity-v1')
    const previousHash = module.hash(vectorInput)
    expect(hex(previousHash)).toBe(
      '3e879ade3dd73ad32ddb31ea61f4d372f8664347c511e368c61ab7e06543de09',
    )
    const header = module.canonicalHeader(
      1,
      1,
      4,
      bytes(0),
      bytes(16),
      42n,
      previousHash,
      encoder.encode('sprout/interop/v1'),
    )
    expect(header).toHaveLength(102)
    expect(hex(module.hash(header))).toBe(
      'eae34456f7b2ef19986fac6d56435de8a1b3a11068faa542332d0117f5a1fa8a',
    )

    const encrypted = module.encrypt(
      header,
      encoder.encode('wasm generated binding'),
    )
    expect(
      new TextDecoder().decode(
        module.decrypt(encrypted.dek, encrypted.payload, header),
      ),
    ).toBe('wasm generated binding')

    const deviceId = bytes(32)
    const device = module.generateDevicePackage(
      deviceId,
      bytes(48),
      bytes(64),
      bytes(80),
      bytes(96),
    )
    const x25519PublicKey = device.x25519PublicKey
    const mlKem768PublicKey = device.mlKem768PublicKey
    const ed25519PublicKey = device.ed25519PublicKey
    const mlDsa65PublicKey = device.mlDsa65PublicKey
    const x25519PrivateKey = device.x25519PrivateKey
    const mlKem768PrivateKey = device.mlKem768PrivateKey
    const ed25519PrivateKey = device.ed25519PrivateKey
    const mlDsa65PrivateKey = device.mlDsa65PrivateKey
    const message = encoder.encode('dual signature binding')
    const signatureContext = encoder.encode('sprout/signature/v1')
    const signatures = module.signDual(
      ed25519PrivateKey,
      mlDsa65PrivateKey,
      message,
      signatureContext,
    )
    expect(signatures.ed25519).toHaveLength(64)
    expect(signatures.mlDsa65.length).toBeGreaterThan(64)
    expect(
      module.verifyDual(
        ed25519PublicKey,
        signatures.ed25519,
        mlDsa65PublicKey,
        signatures.mlDsa65,
        message,
        signatureContext,
      ),
    ).toBe(true)
    expect(
      module.verifyDual(
        ed25519PublicKey,
        signatures.ed25519,
        mlDsa65PublicKey,
        signatures.mlDsa65,
        message,
        encoder.encode('sprout/signature/other'),
      ),
    ).toBe(false)

    const resourceKey = new Uint8Array(32).fill(0x5a)
    const wrapContext = encoder.encode('sprout/hybrid-wrap/v1')
    const resourceId = bytes(112)
    const wrapped = module.wrapResourceKey(
      resourceKey,
      x25519PublicKey,
      mlKem768PublicKey,
      resourceId,
      deviceId,
      0n,
      new Uint8Array(32),
      wrapContext,
    )
    expect(wrapped.suiteVersion).toBe(0x8001)
    expect(wrapped.auditStatus).toBe('production_audit_required')
    const unwrapped = module.unwrapResourceKey(
      wrapped.envelope,
      x25519PrivateKey,
      mlKem768PrivateKey,
      resourceId,
      deviceId,
      0n,
      new Uint8Array(32),
      wrapContext,
    )
    expect(unwrapped.auditStatus).toBe('production_audit_required')
    expect(unwrapped.resourceKey).toEqual(resourceKey)

    const recoveryContext = encoder.encode('sprout/recovery/vector-v1')
    const recoverySecret = new Uint8Array(32).fill(0x77)
    const split = module.splitRecoverySecretNOfN(
      recoverySecret,
      3,
      recoveryContext,
    )
    expect(split.shareCount).toBe(3)
    const shareSet = new module.RecoveryShareSet()
    for (let position = 0; position < split.shareCount; position += 1) {
      shareSet.addShare(split.share(position))
    }
    const recovered = module.combineRecoverySecretNOfN(
      shareSet,
      recoveryContext,
    )
    expect(recovered.secret).toEqual(recoverySecret)
    const recoveredFromBundle =
      module.combineRecoverySecretBundleNOfN(
        split.bundle,
        recoveryContext,
      )
    expect(recoveredFromBundle.secret).toEqual(recoverySecret)

    configureCryptoModuleForTests(module)
    const pwaSignatures = await signDualForPwa(
      { ed25519PrivateKey, mlDsa65PrivateKey },
      message,
      'sprout/signature/v1',
    )
    expect(
      module.verifyDual(
        ed25519PublicKey,
        pwaSignatures.classicalSignature,
        mlDsa65PublicKey,
        pwaSignatures.postQuantumSignature,
        message,
        signatureContext,
      ),
    ).toBe(true)
    const pwaShares = await splitRecoverySecret(
      recoverySecret,
      3,
      'sprout/recovery/vector-v1',
    )
    expect(
      await combineRecoverySecret(
        pwaShares,
        'sprout/recovery/vector-v1',
      ),
    ).toEqual(recoverySecret)
    const metadata = {
      resourceId: '70717273-7475-7677-7879-7a7b7c7d7e7f',
      recipientDeviceId: '20212223-2425-2627-2829-2a2b2c2d2e2f',
      resourceEpoch: 0n,
      previousEpochHash: new Uint8Array(32),
      context: 'sprout/hybrid-wrap/v1',
    }
    const pwaWrapped = await wrapResourceKeyForRecipient(
      resourceKey,
      { x25519PublicKey, mlKem768PublicKey },
      metadata,
    )
    expect(pwaWrapped.auditStatus).toBe(
      'production_audit_required',
    )
    const pwaUnwrapped = await unwrapResourceKeyForRecipient(
      pwaWrapped.envelope,
      { x25519PrivateKey, mlKem768PrivateKey },
      metadata,
    )
    expect(pwaUnwrapped.auditStatus).toBe(
      'production_audit_required',
    )
    expect(pwaUnwrapped.resourceKey).toEqual(resourceKey)

    const encryptedDocument = await encryptDocument(
      { schema: 1, title: 'resource-key wrapped document' },
      {
        projectId: '00010203-0405-4607-8809-0a0b0c0d0e0f',
        resourceId: '70717273-7475-4677-8879-7a7b7c7d7e7f',
        keyId: '10111213-1415-4617-9819-1a1b1c1d1e1f',
        kind: 'task',
        aggregateVersion: 1,
        keyEpoch: 1,
        resourceKey,
      },
    )
    await expect(
      decryptDocument<{ schema: number; title: string }>(
        encryptedDocument,
        {
          projectId: '00010203-0405-4607-8809-0a0b0c0d0e0f',
          resourceId: '70717273-7475-4677-8879-7a7b7c7d7e7f',
          kind: 'task',
          aggregateVersion: 1,
          keyEpoch: 1,
          resourceKey,
        },
      ),
    ).resolves.toEqual({
      schema: 1,
      title: 'resource-key wrapped document',
    })
    await expect(
      decryptDocument(encryptedDocument, {
        projectId: '00010203-0405-4607-8809-0a0b0c0d0e0f',
        resourceId: '70717273-7475-4677-8879-7a7b7c7d7e7f',
        kind: 'task',
        aggregateVersion: 1,
        keyEpoch: 1,
        resourceKey: new Uint8Array(32).fill(0x33),
      }),
    ).rejects.toThrow()
    configureCryptoModuleForTests()

    recoveredFromBundle.destroy()
    recovered.destroy()
    shareSet.destroy()
    split.destroy()
    unwrapped.destroy()
    wrapped.destroy()
    signatures.destroy()
    device.destroy()
    encrypted.destroy()
  }, 30_000)
})
