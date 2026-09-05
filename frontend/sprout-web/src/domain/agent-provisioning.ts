import type { EncryptedPayloadDto, Uuid } from '../api/contracts'
import { canonicalGovernanceJson, sha256Hex, utf8 } from '../ai/canonical'
import { encryptExistingResource } from './resources'
import type { KeyVault } from '../security/key-vault'
import { base64ToBytes, signDual } from '../security/wasm'

export type AgentAvailability = 'controller_private' | 'project_delegable'

export type AgentActionClass =
  | 'create_task'
  | 'replace_own_task'
  | 'delete_own_task'
  | 'assign_own_task'
  | 'unassign_own_task'
  | 'mark_assigned_done'
  | 'append_assigned_note'
  | 'add_assigned_attachment'
  | 'post_comment'

export interface AgentProvisioningDraft {
  identityHandle: string
  systemPrompt: string
  availability: AgentAvailability
  actions: AgentActionClass[]
}

export interface AgentProvisioningPreview {
  identityHandle: string
  systemPrompt: string
  availability: AgentAvailability
  actions: AgentActionClass[]
}

export interface AgentProvisioningContext {
  projectId: Uuid
  projectScopeId: Uuid
  keyEpoch: number
  controllerIdentityId: Uuid
  controllerDeviceId: Uuid
  vault: KeyVault
}

export const AGENT_ACTION_LABELS: Record<AgentActionClass, string> = {
  create_task: 'Creare task',
  replace_own_task: 'Modificare i propri task',
  delete_own_task: 'Eliminare i propri task',
  assign_own_task: 'Assegnare i propri task',
  unassign_own_task: 'Rimuovere assegnazioni dai propri task',
  mark_assigned_done: 'Completare task assegnati',
  append_assigned_note: 'Aggiungere note ai task assegnati',
  add_assigned_attachment: 'Aggiungere allegati ai task assegnati',
  post_comment: 'Pubblicare commenti',
}

export const AGENT_ACTIONS = Object.keys(AGENT_ACTION_LABELS) as AgentActionClass[]

const COMPILER = {
  compiler_id: 'sprout.local-goal.compiler',
  compiler_version: 1,
  compiler_build_digest_hex:
    '0c675e853701375c7ba5d396f4e1f9b55592339a3a4e45859b9f2c2e8fdbbfc2',
} as const

const COMPILATION_SIGNATURE_CONTEXT = 'sprout-governance-compilation-v1'
const FINAL_APPROVAL_SIGNATURE_CONTEXT = 'sprout-final-prompt-approval-v1'
const ADMIN_CREATION_SIGNATURE_CONTEXT = 'sprout-administrator-agent-creation-v1'

const normalizeHandle = (value: string): string => value.trim().toLocaleLowerCase('en-US')

export const validateAgentProvisioningDraft = (
  draft: AgentProvisioningDraft,
): AgentProvisioningDraft => {
  const identityHandle = normalizeHandle(draft.identityHandle)
  const systemPrompt = draft.systemPrompt.trim()
  if (
    identityHandle.length < 3 ||
    identityHandle.length > 128 ||
    /\s/u.test(identityHandle)
  ) {
    throw new Error('Il nome tecnico deve contenere da 3 a 128 caratteri, senza spazi.')
  }
  if (!systemPrompt) throw new Error('Scrivi le istruzioni dell’agente.')
  if (systemPrompt.length > 12_000) {
    throw new Error('Le istruzioni possono contenere al massimo 12.000 caratteri.')
  }
  const actions = [...new Set(draft.actions)]
  if (actions.length === 0) {
    throw new Error('Seleziona almeno una capacità operativa.')
  }
  if (actions.some((action) => !AGENT_ACTIONS.includes(action))) {
    throw new Error('La proposta contiene una capacità non supportata.')
  }
  return { identityHandle, systemPrompt, availability: draft.availability, actions }
}

export const compileAgentProvisioningPreview = (
  draft: AgentProvisioningDraft,
): AgentProvisioningPreview => {
  const normalized = validateAgentProvisioningDraft(draft)
  return normalized
}

const resourceOperation = (action: AgentActionClass): string | undefined => {
  if (
    action === 'create_task' ||
    action === 'replace_own_task' ||
    action === 'delete_own_task' ||
    action === 'append_assigned_note' ||
    action === 'add_assigned_attachment'
  ) return 'write'
  if (action === 'assign_own_task' || action === 'unassign_own_task') {
    return 'delegate_assigned_work'
  }
  if (action === 'mark_assigned_done') return 'complete_assigned_task'
  if (action === 'post_comment') return 'post_comment'
  return undefined
}

const opaqueAgentPayload = (payload: EncryptedPayloadDto) => ({
  version: payload.version,
  algorithm: payload.algorithm,
  key_id: payload.key_id,
  nonce: Array.from(base64ToBytes(payload.nonce_b64)),
  ciphertext: Array.from(base64ToBytes(payload.ciphertext_b64)),
})

const signStatement = async (
  context: AgentProvisioningContext,
  statement: unknown,
  signatureContext: string,
) => {
  const signatures = await signDual(
    context.vault.deviceSecrets,
    canonicalGovernanceJson(statement),
    signatureContext,
  )
  return {
    signer_identity_id: context.controllerIdentityId,
    signer_device_id: context.controllerDeviceId,
    signer_device_key_version: context.vault.deviceSecrets.keyVersion,
    classical_signature: Array.from(signatures.classicalSignature),
    post_quantum_signature: Array.from(signatures.postQuantumSignature),
  }
}

export const buildAgentProvisioningEnvelope = async (
  draft: AgentProvisioningDraft,
  context: AgentProvisioningContext,
): Promise<Record<string, unknown>> => {
  const proposal = compileAgentProvisioningPreview(draft)
  const agentId = crypto.randomUUID()
  const agentPrincipalIdentityId = crypto.randomUUID()
  const localGoalId = crypto.randomUUID()
  const compilationCertificateId = crypto.randomUUID()
  const administratorApprovalId = crypto.randomUUID()
  const draftId = crypto.randomUUID()
  const obligationId = crypto.randomUUID()
  const goalId = crypto.randomUUID()
  const runnerId = crypto.randomUUID()
  const runnerDeviceId = crypto.randomUUID()
  const languageTaskId = crypto.randomUUID()

  const [encryptedPromptDto, encryptedProfileDto, encryptedRunnerLabelDto] =
    await Promise.all([
      encryptExistingResource(context.vault, {
        projectId: context.projectId,
        resourceId: context.projectScopeId,
        kind: 'project',
        aggregateVersion: 1,
        keyEpoch: context.keyEpoch,
        document: { schema: 1, systemPrompt: proposal.systemPrompt },
      }),
      encryptExistingResource(context.vault, {
        projectId: context.projectId,
        resourceId: context.projectScopeId,
        kind: 'project',
        aggregateVersion: 1,
        keyEpoch: context.keyEpoch,
        document: { schema: 1, identityHandle: proposal.identityHandle },
      }),
      encryptExistingResource(context.vault, {
        projectId: context.projectId,
        resourceId: context.projectScopeId,
        kind: 'project',
        aggregateVersion: 1,
        keyEpoch: context.keyEpoch,
        document: { schema: 1, label: `${proposal.identityHandle} runner` },
      }),
    ])
  const encryptedPrompt = opaqueAgentPayload(encryptedPromptDto)
  const encryptedProfile = opaqueAgentPayload(encryptedProfileDto)
  const encryptedRunnerLabel = opaqueAgentPayload(encryptedRunnerLabelDto)
  const promptCommitmentHex = await sha256Hex(utf8(proposal.systemPrompt))
  const ciphertextCommitmentHex = await sha256Hex(
    utf8(JSON.stringify(encryptedPrompt)),
  )
  const allowedOperations = [...new Set(
    proposal.actions.flatMap((action) => {
      const operation = resourceOperation(action)
      return operation ? [operation] : []
    }),
  )]
  const contract = {
    goal: goalId,
    scope: context.projectScopeId,
    obligations: [{
      id: obligationId,
      goal: goalId,
      owner: agentPrincipalIdentityId,
      activation: { kind: 'always' },
      required_for_completion: { kind: 'always' },
      dependency_rank: 0,
    }],
    dependencies: [],
    work_specs: [{
      id: 1,
      obligation: obligationId,
      owner: agentPrincipalIdentityId,
      kind: 'agent_action',
      activation: { kind: 'always' },
      allowed_actions: proposal.actions,
      max_instances: 1,
      max_attempts: 1,
      max_resolution_ticks: 10,
      generation_rank: 0,
      is_entry: true,
      continuations: [],
      failure_plan: { kind: 'fail_goal' },
    }],
    evidence_rules: [{
      id: 1,
      obligation: obligationId,
      kind: 'derived_fact',
      subject: { kind: 'derived' },
      verification: 'semantic_judgment',
    }],
    waiting_rules: [],
    completion_condition: { kind: 'always' },
  }
  const output = {
    contract,
    requirements: [{
      id: 1,
      scope: context.projectScopeId,
      required_actions: proposal.actions,
      required_tools: [],
      required_for_completion: true,
    }],
    bindings: [{ requirement_id: 1, obligation: obligationId, work_spec_id: 1 }],
    security_policies: [{
      work_spec_id: 1,
      allowed_operations: allowedOperations,
      allowed_tools: [],
    }],
  }
  const compilationEnvelope = {
    language_task: {
      id: languageTaskId,
      kind: 'compile_goal_contract',
      input_item_count: 1,
      max_input_items: 1,
      max_output_items: 8,
      max_nesting_depth: 8,
      max_attempts: 1,
      closed_output_schema: true,
      grounded_identifiers_only: true,
      requires_formal_proof: false,
      requires_permission_decision: false,
      requires_exact_semantic_equivalence: false,
      requires_exhaustive_world_knowledge: false,
      allowed_resource_ids: [context.projectScopeId],
      allowed_principal_ids: [agentPrincipalIdentityId, context.controllerIdentityId],
      allowed_tools: [],
    },
    agent: agentPrincipalIdentityId,
    controller: context.controllerIdentityId,
    project_scope: context.projectScopeId,
    allowed_actions: proposal.actions,
    max_requirements: 8,
    max_obligations: 8,
    max_work_specs: 8,
    max_dependencies: 8,
  }
  const outputHashHex = await sha256Hex(canonicalGovernanceJson(output))
  const envelopeHashHex = await sha256Hex(canonicalGovernanceJson(compilationEnvelope))
  const compilationStatement = {
    certificate_id: compilationCertificateId,
    compiler: COMPILER,
    project_id: context.projectId,
    local_goal_id: localGoalId,
    local_revision: 1,
    draft_id: draftId,
    agent_principal_identity_id: agentPrincipalIdentityId,
    controller_identity_id: context.controllerIdentityId,
    prompt_commitment_hex: promptCommitmentHex,
    ciphertext_commitment_hex: ciphertextCommitmentHex,
    output,
    output_hash_hex: outputHashHex,
    envelope: compilationEnvelope,
    envelope_hash_hex: envelopeHashHex,
    authorization: {
      kind: 'administrator_creation',
      approval_id: administratorApprovalId,
    },
    idempotency_key: crypto.randomUUID(),
  }
  const localContract = {
    id: localGoalId,
    revision: 1,
    agent: agentPrincipalIdentityId,
    controller: context.controllerIdentityId,
    encrypted_prompt: encryptedPrompt,
    contract,
    clauses: [{ id: 1, domain: 1, scope: context.projectScopeId, work_spec_ids: [1] }],
    origin: { kind: 'administrator_creation', approval_id: administratorApprovalId },
    supersedes_revision: null,
  }
  const contractHashHex = await sha256Hex(canonicalGovernanceJson(localContract))
  const proposalBinding = {
    project_id: context.projectId,
    administrator_identity_id: context.controllerIdentityId,
    proposed_agent_identity_id: agentPrincipalIdentityId,
    governed_agent_id: agentId,
    proposal_draft_id: draftId,
    local_goal_id: localGoalId,
    local_goal_revision: 1,
    contract_hash_hex: contractHashHex,
    compilation_certificate_id: compilationCertificateId,
    prompt_plaintext_commitment_hex: promptCommitmentHex,
    ciphertext_commitment_hex: ciphertextCommitmentHex,
    availability: proposal.availability,
    scope: context.projectScopeId,
  }
  const administratorStatement = {
    approval_id: administratorApprovalId,
    project_id: context.projectId,
    administrator_identity_id: context.controllerIdentityId,
    signer_device_id: context.controllerDeviceId,
    signer_device_key_version: context.vault.deviceSecrets.keyVersion,
    proposed_agent_identity_id: agentPrincipalIdentityId,
    governed_agent_id: agentId,
    proposal_draft_id: draftId,
    local_goal_id: localGoalId,
    local_goal_revision: 1,
    contract_hash_hex: contractHashHex,
    compilation_certificate_id: compilationCertificateId,
    prompt_plaintext_commitment_hex: promptCommitmentHex,
    ciphertext_commitment_hex: ciphertextCommitmentHex,
    availability: proposal.availability,
    scope: context.projectScopeId,
    canonical_proposal_hash_hex: await sha256Hex(canonicalGovernanceJson(proposalBinding)),
    idempotency_key: crypto.randomUUID(),
  }
  const finalApprovalId = crypto.randomUUID()
  const finalIdempotencyKey = crypto.randomUUID()
  const approvalIdentity = {
    signature_context: FINAL_APPROVAL_SIGNATURE_CONTEXT,
    approval_id: finalApprovalId,
    project_id: context.projectId,
    draft_id: draftId,
    agent_principal_identity_id: agentPrincipalIdentityId,
    controller_identity_id: context.controllerIdentityId,
    local_goal_id: localGoalId,
    local_revision: 1,
    prompt_commitment_hex: promptCommitmentHex,
    ciphertext_commitment_hex: ciphertextCommitmentHex,
    compilation_certificate_id: compilationCertificateId,
    structured_output_hash_hex: outputHashHex,
    idempotency_key: finalIdempotencyKey,
  }
  const finalStatement = {
    approval_id: finalApprovalId,
    project_id: context.projectId,
    draft_id: draftId,
    agent_principal_identity_id: agentPrincipalIdentityId,
    controller_identity_id: context.controllerIdentityId,
    local_goal_id: localGoalId,
    local_revision: 1,
    prompt_commitment_hex: promptCommitmentHex,
    ciphertext_commitment_hex: ciphertextCommitmentHex,
    compilation_certificate_id: compilationCertificateId,
    structured_output_hash_hex: outputHashHex,
    approval_identity_hash_hex: await sha256Hex(canonicalGovernanceJson(approvalIdentity)),
    idempotency_key: finalIdempotencyKey,
  }
  return {
    id: agentId,
    principal_identity_id: agentPrincipalIdentityId,
    controller_identity_id: context.controllerIdentityId,
    identity_handle: proposal.identityHandle,
    encrypted_profile: encryptedProfile,
    profile_resource_node_id: context.projectScopeId,
    key_epoch: context.keyEpoch,
    availability: proposal.availability,
    runner_id: runnerId,
    runner_device_id: runnerDeviceId,
    encrypted_runner_label: encryptedRunnerLabel,
    initial_local_goal: {
      encrypted_prompt: encryptedPrompt,
      supersedes_revision: null,
      compilation: {
        statement: compilationStatement,
        signatures: await signStatement(
          context,
          compilationStatement,
          COMPILATION_SIGNATURE_CONTEXT,
        ),
      },
    },
    final_prompt_approval: {
      statement: finalStatement,
      signatures: await signStatement(
        context,
        finalStatement,
        FINAL_APPROVAL_SIGNATURE_CONTEXT,
      ),
    },
    administrator_creation_approval: {
      statement: administratorStatement,
      signatures: await signStatement(
        context,
        administratorStatement,
        ADMIN_CREATION_SIGNATURE_CONTEXT,
      ),
    },
  }
}
