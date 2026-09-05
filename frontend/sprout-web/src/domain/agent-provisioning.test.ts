import { describe, expect, it } from 'vitest'
import { compileAgentProvisioningPreview } from './agent-provisioning'

describe('agent provisioning compiler boundary', () => {
  it('keeps operational authority separate from natural-language instructions', () => {
    const preview = compileAgentProvisioningPreview({
      identityHandle: '  Project-Helper  ',
      systemPrompt: 'Crea task quando richiesto, ma non eliminare mai un task.',
      availability: 'controller_private',
      actions: ['post_comment'],
    })

    expect(preview.identityHandle).toBe('project-helper')
    expect(preview.actions).toEqual(['post_comment'])
    expect(preview.systemPrompt).toContain('non eliminare mai')
  })

  it('requires an explicit capability selection', () => {
    expect(() => compileAgentProvisioningPreview({
      identityHandle: 'project-helper',
      systemPrompt: 'Aiuta il team.',
      availability: 'controller_private',
      actions: [],
    })).toThrow('Seleziona almeno una capacità operativa')
  })
})
