export const OLLAMA_CHECKPOINT_MODEL = 'qwen2.5:0.5b-instruct'

export interface OllamaLifecycle {
  detect(): Promise<{ installed: boolean; version?: string; models: string[] }>
  /** Native edge runtime selects the official platform distribution. */
  installOfficialDistribution(): Promise<{ installed: true; version: string }>
  pullModel(model: string): Promise<void>
  removeModel(model: string): Promise<void>
}

export const installOllamaWithConsent = async (
  lifecycle: OllamaLifecycle,
  consented: boolean,
): Promise<boolean> => {
  const before = await lifecycle.detect()
  if (before.installed) return true
  if (!consented) return false
  await lifecycle.installOfficialDistribution()
  const after = await lifecycle.detect()
  if (!after.installed) throw new Error('Ollama was not detected after official installation')
  return true
}

export const pullOllamaModelWithSeparateConsent = async (
  lifecycle: OllamaLifecycle,
  model: string,
  consented: boolean,
): Promise<boolean> => {
  if (!consented) return false
  await lifecycle.pullModel(model)
  return true
}
