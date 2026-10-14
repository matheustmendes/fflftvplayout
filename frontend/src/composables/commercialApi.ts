import { authFetch, authFetchRaw } from '@/composables/authFetch'
import { useConfig } from '@/stores/config'

const JSON_HEADERS = { 'content-type': 'application/json;charset=UTF-8' }

/** Weekday names of the ISO numbers 1 (Monday) to 7 (Sunday) live in the locale files. */
export const WEEKDAYS = [1, 2, 3, 4, 5, 6, 7]

/** Today's date (YYYY-MM-DD) in the timezone of the channel. */
export function todayInChannel(timezone: string): string {
    try {
        return new Intl.DateTimeFormat('en-CA', { timeZone: timezone }).format(new Date())
    } catch {
        return new Date().toISOString().slice(0, 10)
    }
}

export function addDays(date: string, days: number): string {
    const value = new Date(`${date}T12:00:00Z`)

    value.setUTCDate(value.getUTCDate() + days)

    return value.toISOString().slice(0, 10)
}

/** 2026-12-10 -> 10/12/2026 */
export function shortDate(date: string | null | undefined): string {
    if (!date) {
        return ''
    }

    const [year, month, day] = date.split('-')

    return `${day}/${month}/${year}`
}

/** `HH:MM:SS` of a UTC timestamp in the timezone of the channel. */
export function clockIn(timestamp: string | null | undefined, timezone: string): string {
    if (!timestamp) {
        return ''
    }

    try {
        return new Intl.DateTimeFormat('pt-BR', {
            timeZone: timezone,
            hour: '2-digit',
            minute: '2-digit',
            second: '2-digit',
            hour12: false,
        }).format(new Date(timestamp))
    } catch {
        return timestamp.slice(11, 19)
    }
}

/** 125 -> 2:05 ; 3725 -> 1:02:05 */
export function formatDuration(seconds: number | null | undefined): string {
    if (seconds == null || !Number.isFinite(seconds)) {
        return ''
    }

    const total = Math.round(seconds)
    const hours = Math.floor(total / 3600)
    const minutes = Math.floor((total % 3600) / 60)
    const rest = String(total % 60).padStart(2, '0')

    return hours > 0 ? `${hours}:${String(minutes).padStart(2, '0')}:${rest}` : `${minutes}:${rest}`
}

export function useCommercialApi() {
    const configStore = useConfig()

    const channelId = () => configStore.channels[configStore.i]?.id
    const base = () => `/api/commercials/${channelId()}`

    function request<T>(method: string, path: string, body?: unknown) {
        return authFetch<T>(`${base()}${path}`, {
            method,
            headers: body === undefined ? undefined : JSON_HEADERS,
            body: body === undefined ? undefined : JSON.stringify(body),
        })
    }

    const get = <T>(path: string) => request<T>('GET', path)

    /** Save a report as a file the operator can send on. */
    async function download(path: string, fallbackName: string) {
        const response = await authFetchRaw(`${base()}${path}`)

        if (!response.ok) {
            throw new Error((await response.text()) || response.statusText)
        }

        const disposition = response.headers.get('content-disposition') ?? ''
        const name = /filename="([^"]+)"/.exec(disposition)?.[1] ?? fallbackName
        const url = URL.createObjectURL(await response.blob())
        const link = document.createElement('a')

        link.href = url
        link.download = name
        link.click()
        URL.revokeObjectURL(url)
    }

    return {
        channelId,
        get,
        download,
        advertisers: (includeArchived = false) =>
            get<CommercialAdvertiser[]>(`/advertisers?include_archived=${includeArchived}`),
        saveAdvertiser: (item: Partial<CommercialAdvertiser>) =>
            item.id
                ? request<CommercialAdvertiser>('PUT', `/advertisers/${item.id}`, item)
                : request<CommercialAdvertiser>('POST', '/advertisers', item),
        archiveAdvertiser: (id: number) => request<void>('DELETE', `/advertisers/${id}`),
        campaigns: (includeArchived = false) =>
            get<CommercialCampaign[]>(`/campaigns?include_archived=${includeArchived}`),
        saveCampaign: (item: Partial<CommercialCampaign>) =>
            item.id
                ? request<CommercialCampaign>('PUT', `/campaigns/${item.id}`, item)
                : request<CommercialCampaign>('POST', '/campaigns', item),
        archiveCampaign: (id: number) => request<void>('DELETE', `/campaigns/${id}`),
        spots: () => get<CommercialSpot[]>('/spots'),
        addSpot: (campaignId: number, title: string, filePath: string) =>
            request<CommercialSpot>('POST', '/spots', { campaign_id: campaignId, title, file_path: filePath }),
        retrySpot: (id: number) => request<CommercialSpot>('POST', `/spots/${id}/retry`),
        removeSpot: (id: number) => request<void>('DELETE', `/spots/${id}`),
        breakWindows: () => get<CommercialBreakWindow[]>('/break-windows'),
        saveBreakWindow: (item: Partial<CommercialBreakWindow>) =>
            item.id
                ? request<CommercialBreakWindow>('PUT', `/break-windows/${item.id}`, item)
                : request<CommercialBreakWindow>('POST', '/break-windows', item),
        removeBreakWindow: (id: number) => request<void>('DELETE', `/break-windows/${id}`),
        weekDay: (weekday: number) => get<CommercialWeekItem[]>(`/week-template?weekday=${weekday}`),
        saveWeekDay: (weekday: number, items: { source: string; title: string }[]) =>
            request<CommercialWeekItem[]>('PUT', `/week-template/${weekday}`, { items }),
        plan: (date: string) => get<PlanPreview>(`/plan/${date}`),
        applyPlan: (date: string, overwrite: boolean) =>
            request<{ message: string; warnings: PlanWarning[] }>('POST', `/plan/${date}?overwrite=${overwrite}`),
        airlog: (from: string, to: string) => get<Airing[]>(`/airlog?date_from=${from}&date_to=${to}&limit=2000`),
        settings: () => get<CommercialSettings>('/settings'),
        saveSettings: (settings: CommercialSettings) => request<CommercialSettings>('PUT', '/settings', settings),
        reportSettings: () => get<CommercialReportSettings>('/report-settings'),
        saveReportSettings: (settings: CommercialReportSettings) =>
            request<CommercialReportSettings>('PUT', '/report-settings', settings),
        autoPlan: () => get<AutoPlanStatus>('/auto-plan'),
        saveAutoPlan: (settings: AutoPlanSettings) => request<AutoPlanSettings>('PUT', '/auto-plan', settings),
        runAutoPlan: () => request<AutoPlanRun[]>('POST', '/auto-plan/run'),
        reportSummary: (query: string) => get<CommercialReport>(`/reports/summary?${query}`),
    }
}
