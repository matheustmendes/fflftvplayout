import { authFetch } from '@/composables/authFetch'

const JSON_HEADERS = { 'content-type': 'application/json;charset=UTF-8' }

export type RelayProtocol = 'rtmp' | 'srt'
export type ConnectionState = 'connected' | 'disconnected' | 'unknown'

export interface DestinationStatus {
    id: number
    name: string
    protocol: RelayProtocol
    state: ConnectionState
}

export interface ChannelHealth {
    filler: { path: string; ok: boolean }
    relay: {
        configured: boolean
        reachable: boolean
        destinations: DestinationStatus[]
        others: number
    }
}

export interface MonitorDestination {
    id?: number
    name: string
    protocol: RelayProtocol
    match_user: string
    match_address: string
    enabled: boolean
}

export interface RelayConfig {
    api_url: string
    path: string
    destinations: MonitorDestination[]
}

export const healthApi = {
    health: (channel: number) => authFetch<ChannelHealth>(`/api/health/${channel}`),
    relay: (channel: number) => authFetch<RelayConfig>(`/api/relay/${channel}`),
    saveRelay: (channel: number, config: RelayConfig) =>
        authFetch<RelayConfig>(`/api/relay/${channel}`, {
            method: 'PUT',
            headers: JSON_HEADERS,
            body: JSON.stringify(config),
        }),
}
