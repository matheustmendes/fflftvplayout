import { AuthFetchError } from '@/composables/authFetch'
import { i18n } from '@/i18n'

/**
 * An error as a person who does not know what an HTTP status is can read it:
 * what happened in plain words, then the technical detail from the server.
 */
export function friendlyError(error: unknown): string {
    const detail = error instanceof Error ? error.message : String(error)

    if (error instanceof AuthFetchError) {
        const status = error.response.status
        const key = [401, 403, 404, 409, 413, 429].includes(status) ? String(status) : status >= 500 ? '500' : 'other'
        const plain = i18n.t(`errors.${key}`)

        // The server explains 400 and 409 best; for the others the plain text is enough.
        return [400, 409, 422].includes(status) ? `${plain} ${detail}` : plain
    }

    if (error instanceof TypeError) {
        return i18n.t('errors.network')
    }

    return detail
}
