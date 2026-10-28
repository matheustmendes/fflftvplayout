import { authFetch } from '@/composables/authFetch'

const JSON_HEADERS = { 'content-type': 'application/json;charset=UTF-8' }

export interface YoutubeSource {
    id: number
    channel_id: number
    youtube_channel_id: string
    name: string
}

export type YoutubeJobStatus = 'running' | 'done' | 'failed' | 'cancelled'

export interface YoutubeJob {
    id: string
    url: string
    folder: string
    title: string
    status: YoutubeJobStatus
    playlist_index: number
    playlist_total: number
    item_index: number
    item_total: number
    current: string
    percent: number
    downloaded: number
    already_had: number
    rejected: number
    failed: number
    errors: string[]
    started_at: number
    finished_at: number | null
}

export interface YoutubeView {
    /** Version of yt-dlp, or null when it is not installed on the server. */
    version: string | null
    cookies: boolean
    sources: YoutubeSource[]
    jobs: YoutubeJob[]
}

export interface YoutubeVideo {
    id: string
    title: string
    duration: number | null
}

export interface YoutubePreview {
    kind: 'video' | 'playlist' | 'collection'
    title: string
    owner_id: string
    owner_name: string
    videos: YoutubeVideo[]
    playlists: { url: string; title: string }[]
    allowed: boolean
    default_folder: string
}

export const YOUTUBE_HEIGHTS = [1080, 720, 480] as const

export const youtubeApi = {
    get: (channel: number) => authFetch<YoutubeView>(`/api/youtube/${channel}`),
    jobs: (channel: number) => authFetch<YoutubeJob[]>(`/api/youtube/${channel}/jobs`),
    addSource: (channel: number, url: string) =>
        authFetch<YoutubeSource[]>(`/api/youtube/${channel}/sources`, {
            method: 'POST',
            headers: JSON_HEADERS,
            body: JSON.stringify({ url }),
        }),
    removeSource: (channel: number, id: number) =>
        authFetch<YoutubeSource[]>(`/api/youtube/${channel}/sources/${id}`, { method: 'DELETE' }),
    preview: (channel: number, url: string) =>
        authFetch<YoutubePreview>(`/api/youtube/${channel}/preview`, {
            method: 'POST',
            headers: JSON_HEADERS,
            body: JSON.stringify({ url }),
        }),
    start: (channel: number, url: string, folder: string, height: number) =>
        authFetch<YoutubeJob>(`/api/youtube/${channel}/imports`, {
            method: 'POST',
            headers: JSON_HEADERS,
            body: JSON.stringify({ url, folder, height }),
        }),
    cancel: (channel: number, id: string) =>
        authFetch<void>(`/api/youtube/${channel}/imports/${id}`, { method: 'DELETE' }),
}
