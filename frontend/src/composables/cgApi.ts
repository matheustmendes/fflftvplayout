import { authFetch } from '@/composables/authFetch'
import { useConfig } from '@/stores/config'

const JSON_HEADERS = { 'content-type': 'application/json;charset=UTF-8' }

export type CgKind = 'text' | 'image'
export type CgMode = 'clock' | 'program'

export interface CgRule {
    id?: number
    mode: CgMode
    /** Monday is bit 0 (1), Sunday bit 6 (64). */
    weekdays: number
    date_from: string | null
    date_to: string | null
    start_time: string
    program_match: string
    offset_seconds: number
    duration_seconds: number
    repeat_every_seconds: number | null
    until_time: string | null
    enabled: boolean
}

export interface Cg {
    id?: number
    name: string
    kind: CgKind
    text: string
    image_path: string
    x_pct: number
    y_pct: number
    size_pct: number
    font_family: string
    font_weight: 'normal' | 'semibold' | 'bold'
    text_color: string
    box_enabled: boolean
    box_color: string
    box_opacity: number
    opacity: number
    fade_in_seconds: number
    enabled: boolean
    schedules: CgRule[]
}

export interface CgOverview {
    items: Cg[]
    on_air: number[]
}

export const ALL_DAYS = 127
export const WORK_DAYS = 31

export function newRule(): CgRule {
    return {
        mode: 'clock',
        weekdays: ALL_DAYS,
        date_from: null,
        date_to: null,
        start_time: '12:00',
        program_match: '',
        offset_seconds: 0,
        duration_seconds: 15,
        repeat_every_seconds: null,
        until_time: null,
        enabled: true,
    }
}

export function newCg(): Cg {
    return {
        name: '',
        kind: 'text',
        text: '',
        image_path: '',
        x_pct: 5,
        y_pct: 85,
        size_pct: 5,
        font_family: '',
        font_weight: 'bold',
        text_color: '#ffffff',
        box_enabled: true,
        box_color: '#000000',
        box_opacity: 0.6,
        opacity: 1,
        fade_in_seconds: 0.5,
        enabled: true,
        schedules: [newRule()],
    }
}

/** `18:00:00` -> `18:00`, what a time input and the operator expect. */
export function shortClock(value: string | null | undefined): string {
    return (value ?? '').slice(0, 5)
}

export function useCgApi() {
    const configStore = useConfig()

    const base = () => `/api/cg/${configStore.channels[configStore.i]?.id}`

    function request<T>(method: string, path: string, body?: unknown) {
        return authFetch<T>(`${base()}${path}`, {
            method,
            headers: body === undefined ? undefined : JSON_HEADERS,
            body: body === undefined ? undefined : JSON.stringify(body),
        })
    }

    return {
        overview: () => request<CgOverview>('GET', ''),
        save: (item: Cg) => (item.id ? request<Cg>('PUT', `/${item.id}`, item) : request<Cg>('POST', '', item)),
        remove: (id: number) => request<void>('DELETE', `/${id}`),
        show: (id: number, seconds: number) => request<void>('POST', `/${id}/show`, { seconds }),
        hide: (id: number) => request<void>('DELETE', `/${id}/show`),
    }
}
