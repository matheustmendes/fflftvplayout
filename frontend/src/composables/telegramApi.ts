import { authFetch } from '@/composables/authFetch'

const JSON_HEADERS = { 'content-type': 'application/json;charset=UTF-8' }

export interface TelegramSettings {
    enabled: boolean
    chat_id: string
    alert_output: boolean
    alert_destinations: boolean
    alert_filler: boolean
    alert_playlist: boolean
    alert_commercials: boolean
    alert_disk: boolean
    daily_summary: boolean
    summary_time: string
    commands_enabled: boolean
}

export interface TelegramView {
    /** A token of the bot was saved. It is never sent back. */
    token_set: boolean
    settings: TelegramSettings
}

export const telegramApi = {
    get: (channel: number) => authFetch<TelegramView>(`/api/telegram/${channel}`),
    save: (channel: number, settings: TelegramSettings) =>
        authFetch<TelegramView>(`/api/telegram/${channel}`, {
            method: 'PUT',
            headers: JSON_HEADERS,
            body: JSON.stringify(settings),
        }),
    saveToken: (channel: number, token: string) =>
        authFetch<TelegramView>(`/api/telegram/${channel}/token`, {
            method: 'PUT',
            headers: JSON_HEADERS,
            body: JSON.stringify({ token }),
        }),
    test: (channel: number) => authFetch<{ bot: string }>(`/api/telegram/${channel}/test`, { method: 'POST' }),
}
