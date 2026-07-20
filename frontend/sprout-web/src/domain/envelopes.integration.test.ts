/// <reference types="node" />

// @vitest-environment node

import { readFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { describe, expect, it } from 'vitest'
import type { ProjectDeviceKeyPackage, Uuid } from '../api/contracts'
import type { KeyVault } from '../security/key-vault'
import {
  base64ToBytes,
  bytesToBase64,
  configureCryptoModuleForTests,
  generateDeviceSecrets,
  decryptDocument,
  encryptDocument,
  zeroBytes,
} from '../security/wasm'
import type { GeneratedSproutCryptoModule } from '../security/wasm'
import {
  buildInitialResourceEpoch,
  buildResourceEpochRotation,
  buildResourceKeyEnvelopes,
  importResourceKeyEnvelopes,
} from './envelopes'

const asArrayBuffer = (value: Uint8Array): ArrayBuffer =>
  value.buffer.slice(
    value.byteOffset,
    value.byteOffset + value.byteLength,
  ) as ArrayBuffer

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

const packageView = async (
  identityId: Uuid,
  deviceId: Uuid,
  packageBytes: Uint8Array,
): Promise<ProjectDeviceKeyPackage> => ({
  identity_id: identityId,
  device_id: deviceId,
  key_version: 1,
  generation: 0,
  package_b64: bytesToBase64(packageBytes),
  package_hash_b64: bytesToBase64(
    new Uint8Array(
      await crypto.subtle.digest('SHA-256', asArrayBuffer(packageBytes)),
    ),
  ),
  suite_status: 'experimental_not_production_approved',
})

const fakeVault = (
  identityId: Uuid,
  deviceId: Uuid,
  deviceSecrets: Awaited<ReturnType<typeof generateDeviceSecrets>>,
) => {
  const resourceKeys = new Map<string, Uint8Array>()
  return {
    vault: {
      localIdentityId: identityId,
      localDeviceId: deviceId,
      deviceSecrets,
      getResourceKey: (resourceId: Uuid, epoch = 1) =>
        resourceKeys.get(`body:${resourceId}:${epoch}`),
      getHeaderKey: (resourceId: Uuid, epoch = 1) =>
        resourceKeys.get(`header:${resourceId}:${epoch}`),
      putResourceKey: async (
        resourceId: Uuid,
        key: Uint8Array,
        epoch = 1,
        purpose: 'body' | 'header' = 'body',
      ) => {
        resourceKeys.set(`${purpose}:${resourceId}:${epoch}`, key.slice())
      },
    } as unknown as KeyVault,
    resourceKeys,
  }
}

describe('resource envelope ingestion', () => {
  it('verifies both sender signatures and unwraps for the recipient device', async () => {
    configureCryptoModuleForTests(await loadGeneratedModule())
    const projectId = '11111111-1111-4111-8111-111111111111'
    const resourceId = '22222222-2222-4222-8222-222222222222'
    const senderIdentityId = '33333333-3333-4333-8333-333333333333'
    const senderDeviceId = '44444444-4444-4444-8444-444444444444'
    const recipientIdentityId =
      '55555555-5555-4555-8555-555555555555'
    const recipientDeviceId =
      '66666666-6666-4666-8666-666666666666'
    const senderSecrets = await generateDeviceSecrets(senderDeviceId)
    const recipientSecrets = await generateDeviceSecrets(recipientDeviceId)
    const sender = fakeVault(
      senderIdentityId,
      senderDeviceId,
      senderSecrets,
    )
    const recipient = fakeVault(
      recipientIdentityId,
      recipientDeviceId,
      recipientSecrets,
    )
    const resourceKey = crypto.getRandomValues(new Uint8Array(32))
    try {
      const recipientPackage = await packageView(
        recipientIdentityId,
        recipientDeviceId,
        recipientSecrets.publicPackage,
      )
      const senderPackage = await packageView(
        senderIdentityId,
        senderDeviceId,
        senderSecrets.publicPackage,
      )
      const built = await buildInitialResourceEpoch(sender.vault, {
        projectId,
        resourceId,
        resourceKey,
        recipientIdentityId,
        packages: [recipientPackage],
      })
      const imported = await importResourceKeyEnvelopes(recipient.vault, {
        projectId,
        envelopes: built.envelopes.map((envelope) => ({
          ...envelope,
          sender_identity_id: senderIdentityId,
          sender_device_id: senderDeviceId,
          previous_epoch_hash_b64: null,
        })),
        packages: [senderPackage, recipientPackage],
      })
      expect(imported).toBe(1)
      expect(recipient.resourceKeys.get(`body:${resourceId}:1`)).toEqual(
        resourceKey,
      )
      const previousKeyCommitment = base64ToBytes(
        built.epoch.key_commitment_b64,
      )
      const rotated = await buildResourceEpochRotation(sender.vault, {
        projectId,
        resourceId,
        previousEpochId: built.epoch.id,
        currentEpoch: 1,
        previousKeyCommitment,
        recipientIdentityIds: [recipientIdentityId],
        packages: [recipientPackage],
      })
      try {
        const importedRotation = await importResourceKeyEnvelopes(
          recipient.vault,
          {
            projectId,
            envelopes: rotated.rotation.envelopes.map((envelope) => ({
              ...envelope,
              sender_identity_id: senderIdentityId,
              sender_device_id: senderDeviceId,
              previous_epoch_hash_b64: built.epoch.key_commitment_b64,
            })),
            packages: [senderPackage, recipientPackage],
          },
        )
        expect(importedRotation).toBe(1)
        expect(recipient.resourceKeys.get(`body:${resourceId}:2`)).toEqual(
          rotated.resourceKey,
        )
      } finally {
        zeroBytes(rotated.resourceKey, previousKeyCommitment)
      }
    } finally {
      zeroBytes(
        resourceKey,
        senderSecrets.x25519PrivateKey,
        senderSecrets.mlKem768PrivateKey,
        senderSecrets.ed25519PrivateKey,
        senderSecrets.mlDsa65PrivateKey,
        recipientSecrets.x25519PrivateKey,
        recipientSecrets.mlKem768PrivateKey,
        recipientSecrets.ed25519PrivateKey,
        recipientSecrets.mlDsa65PrivateKey,
      )
      configureCryptoModuleForTests()
    }
  })

  it('proves an assignee receives one task body and only ancestor headers', async () => {
    configureCryptoModuleForTests(await loadGeneratedModule())
    const projectId = '11111111-1111-4111-8111-111111111111'
    const ownerIdentityId = '22222222-2222-4222-8222-222222222222'
    const ownerDeviceId = '33333333-3333-4333-8333-333333333333'
    const assigneeIdentityId = '44444444-4444-4444-8444-444444444444'
    const assigneeDeviceId = '55555555-5555-4555-8555-555555555555'
    const [topicId, listId, taskId, siblingId, descendantId] = [
      '66666666-6666-4666-8666-666666666666',
      '77777777-7777-4777-8777-777777777777',
      '88888888-8888-4888-8888-888888888888',
      '99999999-9999-4999-8999-999999999999',
      'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa',
    ]
    const ownerSecrets = await generateDeviceSecrets(ownerDeviceId)
    const assigneeSecrets = await generateDeviceSecrets(assigneeDeviceId)
    const owner = fakeVault(ownerIdentityId, ownerDeviceId, ownerSecrets)
    const assignee = fakeVault(
      assigneeIdentityId,
      assigneeDeviceId,
      assigneeSecrets,
    )
    const assigneePackage = await packageView(
      assigneeIdentityId,
      assigneeDeviceId,
      assigneeSecrets.publicPackage,
    )
    const ownerPackage = await packageView(
      ownerIdentityId,
      ownerDeviceId,
      ownerSecrets.publicPackage,
    )
    const keys = new Map(
      [topicId, listId, taskId, siblingId, descendantId].map((id) => [
        id,
        {
          body: crypto.getRandomValues(new Uint8Array(32)),
          header: crypto.getRandomValues(new Uint8Array(32)),
        },
      ]),
    )
    try {
      const taskPayload = await encryptDocument(
        { schema: 1, title: 'Assigned task', notes: 'full body' },
        {
          projectId,
          resourceId: taskId,
          keyId: crypto.randomUUID(),
          kind: 'task',
          aggregateVersion: 1,
          keyEpoch: 1,
          resourceKey: keys.get(taskId)!.body,
        },
      )
      const headerPayloads = await Promise.all(
        [topicId, listId].map((resourceId) =>
          encryptDocument(
            { schema: 1, name: resourceId === topicId ? 'Topic' : 'List' },
            {
              projectId,
              resourceId,
              keyId: crypto.randomUUID(),
              kind: resourceId === topicId ? 'topic' : 'task-list',
              aggregateVersion: 1,
              keyEpoch: 1,
              resourceKey: keys.get(resourceId)!.header,
            },
          ),
        ),
      )
      const envelopeDtos = (
        await Promise.all([
          ...[topicId, listId].map((resourceId) =>
            buildResourceKeyEnvelopes(owner.vault, {
              projectId,
              resourceId,
              resourceKey: keys.get(resourceId)!.header,
              keyPurpose: 'header',
              recipientIdentityId: assigneeIdentityId,
              packages: [assigneePackage],
            }),
          ),
          buildResourceKeyEnvelopes(owner.vault, {
            projectId,
            resourceId: taskId,
            resourceKey: keys.get(taskId)!.body,
            recipientIdentityId: assigneeIdentityId,
            packages: [assigneePackage],
          }),
        ])
      ).flat()
      await importResourceKeyEnvelopes(assignee.vault, {
        projectId,
        envelopes: envelopeDtos.map((envelope) => ({
          ...envelope,
          sender_identity_id: ownerIdentityId,
          sender_device_id: ownerDeviceId,
          previous_epoch_hash_b64: null,
        })),
        packages: [ownerPackage, assigneePackage],
      })

      expect(
        await decryptDocument(taskPayload, {
          projectId,
          resourceId: taskId,
          kind: 'task',
          aggregateVersion: 1,
          keyEpoch: 1,
          resourceKey: assignee.vault.getResourceKey(taskId)!,
        }),
      ).toMatchObject({ title: 'Assigned task', notes: 'full body' })
      for (const [index, resourceId] of [topicId, listId].entries()) {
        expect(
          await decryptDocument(headerPayloads[index], {
            projectId,
            resourceId,
            kind: resourceId === topicId ? 'topic' : 'task-list',
            aggregateVersion: 1,
            keyEpoch: 1,
            resourceKey: assignee.vault.getHeaderKey(resourceId)!,
          }),
        ).toHaveProperty('schema', 1)
        expect(assignee.vault.getResourceKey(resourceId)).toBeUndefined()
        // T-LLR-06.6: header key cannot open body ciphertext, even if body bytes leak.
        const ancestorBody = await encryptDocument(
          { schema: 1, title: 'secret body', notes: 'denied' },
          {
            projectId,
            resourceId,
            keyId: crypto.randomUUID(),
            kind: resourceId === topicId ? 'topic' : 'task-list',
            aggregateVersion: 1,
            keyEpoch: 1,
            resourceKey: keys.get(resourceId)!.body,
          },
        )
        await expect(
          decryptDocument(ancestorBody, {
            projectId,
            resourceId,
            kind: resourceId === topicId ? 'topic' : 'task-list',
            aggregateVersion: 1,
            keyEpoch: 1,
            resourceKey: assignee.vault.getHeaderKey(resourceId)!,
          }),
        ).rejects.toThrow()
      }
      for (const inaccessible of [siblingId, descendantId]) {
        expect(assignee.vault.getResourceKey(inaccessible)).toBeUndefined()
        expect(assignee.vault.getHeaderKey(inaccessible)).toBeUndefined()
      }
      expect(
        JSON.stringify({
          topic: { payload: null, header: headerPayloads[0] },
          list: { payload: null, header: headerPayloads[1] },
        }),
      ).not.toContain(taskPayload.ciphertext_b64)
    } finally {
      for (const value of keys.values()) zeroBytes(value.body, value.header)
      zeroBytes(
        ownerSecrets.x25519PrivateKey,
        ownerSecrets.mlKem768PrivateKey,
        ownerSecrets.ed25519PrivateKey,
        ownerSecrets.mlDsa65PrivateKey,
        assigneeSecrets.x25519PrivateKey,
        assigneeSecrets.mlKem768PrivateKey,
        assigneeSecrets.ed25519PrivateKey,
        assigneeSecrets.mlDsa65PrivateKey,
      )
      configureCryptoModuleForTests()
    }
  })
})
