import { ApiError } from '../api/client'

export type AuthPhase = 'signin' | 'signup' | 'verify' | 'recover'

export const authErrorMessage = (
  error: unknown,
  phase: AuthPhase,
): string => {
  if (error instanceof ApiError) {
    if (error.status === 409) {
      return 'Conflitto sul device di sviluppo: ricarica la pagina e riprova «Entra come admin.minerva».'
    }
    if (error.status === 401 && phase === 'signin') {
      return 'Non puoi ancora accedere con passkey: completa prima Crea → Verifica, poi registra una passkey in Security prima di uscire.'
    }
    if (
      error.status === 400 &&
      phase === 'verify' &&
      error.message.includes('already verified')
    ) {
      return 'Questo account è già verificato. Usa Recupera con la tua email oppure Accedi con passkey (dopo averla registrata in Security).'
    }
    if (
      error.status === 400 &&
      phase === 'verify' &&
      error.message.includes('verification token')
    ) {
      return 'Token non valido, già usato o scaduto. Se l’account è già attivo usa Recupera; altrimenti torna su Crea con la stessa email e handle.'
    }
  }
  return error instanceof Error ? error.message : 'Si è verificato un errore imprevisto'
}
