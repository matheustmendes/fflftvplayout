import { computed, ref } from 'vue'
import { defineStore } from 'pinia'

import { useAuth } from '@/stores/auth'

export type ChannelState = 'offline' | 'on_air' | 'live' | 'restarting' | 'stopped'

/** The server sends status about once a second; this long without one is a problem. */
const STALE_AFTER_MS = 6000

/** A channel that was on the air this recently and is not now is being restarted. */
const RESTART_WINDOW_MS = 120_000

/**
 * What the channel is doing right now, for every screen: the one connection to
 * the status stream of the channel lives here so the header, the dashboard and
 * the player do not each open their own.
 */
export const useLive = defineStore('live', () => {
    const now = ref(Date.now())
    const connected = ref(false)
    const running = ref(false)
    const lastMessageAt = ref(0)
    const lastRunningAt = ref(0)
    const media = ref<PlaylistItem | null>(null)
    const index = ref(0)
    const elapsed = ref(0)
    const ingest = ref(false)
    const shift = ref(0)
    const audio = ref<AudioLevel | null>(null)

    let source: EventSource | null = null
    let channelId: number | null = null
    let clock: ReturnType<typeof setInterval> | undefined

    const stale = computed(() => connected.value && now.value - lastMessageAt.value > STALE_AFTER_MS)

    const state = computed<ChannelState>(() => {
        if (!connected.value || stale.value) {
            return 'offline'
        }

        if (running.value) {
            return ingest.value ? 'live' : 'on_air'
        }

        return now.value - lastRunningAt.value < RESTART_WINDOW_MS && lastRunningAt.value > 0 ? 'restarting' : 'stopped'
    })

    const secondsLeft = computed(() => {
        const out = media.value?.out ?? 0
        const seek = media.value?.in ?? 0

        return running.value ? Math.max(0, out - seek - Math.max(0, elapsed.value)) : 0
    })

    const progress = computed(() => {
        const length = (media.value?.out ?? 0) - (media.value?.in ?? 0)

        return running.value && length > 0 ? Math.min(100, Math.max(0, (elapsed.value * 100) / length)) : 0
    })

    function apply(raw: string) {
        lastMessageAt.value = Date.now()

        if (raw === 'connected') {
            return
        }

        try {
            const status = JSON.parse(raw) as PlayoutStatus

            running.value = true
            lastRunningAt.value = Date.now()
            media.value = status.media
            index.value = status.index
            elapsed.value = status.elapsed
            ingest.value = status.ingest
            shift.value = status.shift
            audio.value = status.audio ?? null
        } catch {
            running.value = false
            media.value = null
            ingest.value = false
            audio.value = null
        }
    }

    function stop() {
        source?.close()
        source = null
        channelId = null
        connected.value = false
        running.value = false
        clearInterval(clock)
        clock = undefined
    }

    /** Follow the status of a channel; calling it again for the same channel does nothing. */
    async function start(channel: number) {
        if (source && channelId === channel) {
            return
        }

        stop()
        channelId = channel
        lastRunningAt.value = 0
        lastMessageAt.value = Date.now()

        const auth = useAuth()

        await auth.obtainUuid()

        // The user may have changed channel or logged out while the id was fetched.
        if (channelId !== channel) {
            return
        }

        source = new EventSource(`/data/event/${channel}?endpoint=playout&uuid=${auth.uuid}`)
        source.onopen = () => {
            connected.value = true
        }
        source.onerror = () => {
            // The browser reconnects by itself; a dead id needs a fresh one.
            connected.value = false

            if (source?.readyState === EventSource.CLOSED) {
                const retry = channelId

                stop()

                if (retry !== null) {
                    setTimeout(() => void start(retry), 3000)
                }
            }
        }
        source.onmessage = (event) => {
            connected.value = true
            apply(event.data)
        }
        clock = setInterval(() => (now.value = Date.now()), 1000)
    }

    return {
        state,
        connected,
        running,
        media,
        index,
        elapsed,
        ingest,
        shift,
        audio,
        secondsLeft,
        progress,
        start,
        stop,
    }
})
