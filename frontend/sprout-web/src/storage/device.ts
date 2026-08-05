import type { Uuid } from '../api/contracts'

const DEVICE_ID_KEY = 'sprout.device-id'
const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i

export const getOrCreateDeviceId = (): Uuid => {
  const stored = localStorage.getItem(DEVICE_ID_KEY)
  if (stored && UUID_PATTERN.test(stored)) {
    return stored
  }
  const created = crypto.randomUUID()
  localStorage.setItem(DEVICE_ID_KEY, created)
  return created
}
