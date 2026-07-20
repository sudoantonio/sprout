interface TrustedTypesFactory {
  createPolicy(
    name: string,
    rules: { createScriptURL(value: string): string },
  ): { createScriptURL(value: string): unknown }
}

const serviceWorkerUrl = (): string => {
  const trustedTypes = Reflect.get(window, 'trustedTypes') as
    | TrustedTypesFactory
    | undefined
  if (!trustedTypes) return '/sw.js'
  const policy = trustedTypes.createPolicy('sprout', {
    createScriptURL: (value) => {
      if (value !== '/sw.js') {
        throw new TypeError('Untrusted service-worker URL')
      }
      return value
    },
  })
  return policy.createScriptURL('/sw.js') as string
}

export const registerServiceWorker = async (): Promise<
  ServiceWorkerRegistration | undefined
> => {
  if (!('serviceWorker' in navigator) || !import.meta.env.PROD) {
    return undefined
  }

  return navigator.serviceWorker.register(serviceWorkerUrl(), {
    scope: '/',
    updateViaCache: 'none',
  })
}

export const requestPersistentStorage = async (): Promise<boolean> => {
  if (!navigator.storage?.persist) {
    return false
  }

  if (await navigator.storage.persisted?.()) {
    return true
  }
  return navigator.storage.persist()
}
