<script setup lang="ts">
import dayjs from 'dayjs'
import customParseFormat from 'dayjs/plugin/customParseFormat.js'
import LocalizedFormat from 'dayjs/plugin/localizedFormat.js'
import timezone from 'dayjs/plugin/timezone.js'
import utc from 'dayjs/plugin/utc.js'
import mpegts from 'mpegts.js'

import { computed, ref, onMounted, onBeforeUnmount, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { throttle } from 'es-toolkit/function'
import { storeToRefs } from 'pinia'
import { useEventSource } from '@vueuse/core'

import { authFetch } from '@/composables/authFetch'
import { displayTitle } from '@/composables/displayTitle'
import { stringFormatter } from '@/composables/helper'
import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'
import { useConfig } from '@/stores/config'
import { useLive } from '@/stores/live'
import { usePlaylist } from '@/stores/playlist'

import 'dayjs/locale/de'
import 'dayjs/locale/en'
import 'dayjs/locale/es'
import 'dayjs/locale/pt-br'
import 'dayjs/locale/ru'

dayjs.extend(customParseFormat)
dayjs.extend(LocalizedFormat)
dayjs.extend(timezone)
dayjs.extend(utc)

import ConfirmModal from '@/components/ui/ConfirmModal.vue'
import StatusPill from '@/components/ui/StatusPill.vue'
import VideoPlayer from '@/components/utils/VideoPlayer.vue'

const { t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const playlistStore = usePlaylist()
const live = useLive()
const { filename, secToHMS } = stringFormatter()
const { i } = storeToRefs(useConfig())

const currentDefault = {
    uid: '',
    title: t('control.noClip'),
    source: t('control.noClip'),
    duration: 0,
    in: 0,
    out: 0,
    ad: false,
} as PlaylistItem

playlistStore.current = currentDefault

const timeStr = ref('00:00:00')
const timer = ref()
const errorCounter = ref(0)
const volumeLevel = ref(configStore.playout.audio?.volume ?? 1)
const streamExtension = ref(configStore.channels[configStore.i]?.preview_url.split('.').pop())
const httpStreamFlv = ref(null)
const httpFlvSource = ref({
    type: 'flv',
    isLive: true,
    url: configStore.channels[configStore.i]?.preview_url,
})
const mpegtsOptions = ref({
    lazyLoadMaxDuration: 3 * 60,
    liveBufferLatencyChasing: true,
})

const streamUrl = ref(`/data/event/${configStore.channels[configStore.i]?.id}?endpoint=playout&uuid=${authStore.uuid}`)

const activeIngestLabel = computed(() => {
    const id = playlistStore.ingestListenerId

    if (id === null) return t('control.ingest')

    const backend = playlistStore.ingestListenerBackend?.toUpperCase()
    const name = playlistStore.ingestListenerName?.trim()
    const details = name && backend ? `${name} (${backend} #${id})` : backend ? `${backend} #${id}` : `#${id}`

    return `${t('control.ingest')}: ${details}`
})

// 'http://127.0.0.1:8787/data/event/1?endpoint=playout&uuid=f2f8c29b-712a-48c5-8919-b535d3a05a3a'
const { status, data, error, close } = useEventSource(streamUrl, [], {
    autoReconnect: {
        retries: -1,
        delay: 1000,
        onFailed() {
            indexStore.sseConnected = false
        },
    },
})

onMounted(() => {
    let player: any = null

    if (streamExtension.value === 'flv' && mpegts.getFeatureList().mseLivePlayback) {
        if (typeof player !== 'undefined' && player != null) {
            player.unload()
            player.detachMediaElement()
            player.destroy()
            player = null
        }

        player = mpegts.createPlayer(httpFlvSource.value, mpegtsOptions.value)
        player.attachMediaElement(httpStreamFlv.value)
        player.load()
    }

    clock()
})

onBeforeUnmount(() => {
    indexStore.sseConnected = false
    close()

    if (timer.value) {
        clearTimeout(timer.value)
    }
})

watch([status, error], async () => {
    if (status.value === 'OPEN') {
        indexStore.sseConnected = true
        errorCounter.value = 0
    } else {
        indexStore.sseConnected = false
        errorCounter.value += 1

        if (errorCounter.value > 11) {
            await authStore.obtainUuid()
            streamUrl.value = `/data/event/${configStore.channels[configStore.i]?.id}?endpoint=playout&uuid=${
                authStore.uuid
            }`
            errorCounter.value = 0
        }
    }
})

watch([data], () => {
    if (data.value && data.value !== 'connected') {
        try {
            const playout_status = JSON.parse(data.value)
            playlistStore.setStatus(playout_status)
        } catch {
            indexStore.sseConnected = true
            playlistStore.playoutIsRunning = false
            resetStatus()
        }
    }
})

watch([i], () => {
    resetStatus()
    volumeLevel.value = configStore.playout.audio?.volume ?? 1

    streamUrl.value = `/data/event/${configStore.channels[configStore.i]?.id}?endpoint=playout&uuid=${authStore.uuid}`

    if (timer.value) {
        clearTimeout(timer.value)
    }
})

function timeRemaining() {
    let remaining = playlistStore.current.out - playlistStore.elapsedSec

    if (remaining < 0) {
        remaining = 0
    }

    return remaining
}

function audioMeterValue() {
    const peak = playlistStore.audioLevel?.peak_db ?? -100

    return Math.min(100, Math.max(0, ((peak + 60) / 60) * 100))
}

function volumeIcon() {
    if (volumeLevel.value <= 0) {
        return 'bi-volume-mute'
    }

    if (volumeLevel.value < 1) {
        return 'bi-volume-off'
    }

    if (volumeLevel.value < 2) {
        return 'bi-volume-down'
    }

    return 'bi-volume-up'
}

const applyVolumeControl = throttle(async () => {
    const volume = Math.min(1.5, Math.max(0, Number(volumeLevel.value) || 0))
    volumeLevel.value = volume
    configStore.playout.audio.volume = volume

    try {
        await configStore.applyAudioEffects(volume)
    } catch (error) {
        indexStore.msgAlert('error', error instanceof Error ? error.message : String(error), 3)
    }
}, 250)

function muteAudio() {
    if (configStore.playout.audio.volume === 0) {
        volumeLevel.value = 1
    } else {
        volumeLevel.value = 0
    }

    configStore.playout.audio.volume = volumeLevel.value
    configStore.applyAudioEffects(volumeLevel.value)
}

async function clock() {
    async function setTime(resolve: any) {
        timeStr.value = dayjs().tz(configStore.timezone).format('HH:mm:ss')
        timer.value = setTimeout(() => setTime(resolve), 1000)
    }
    return new Promise((resolve) => setTime(resolve))
}

function resetStatus() {
    playlistStore.ingestRuns = false
    playlistStore.ingestListenerId = null
    playlistStore.ingestListenerName = null
    playlistStore.ingestListenerBackend = null
    playlistStore.elapsedSec = 0
    playlistStore.shift = 0
    playlistStore.audioLevel = null
    playlistStore.current = currentDefault
}

const controlProcess = throttle(async (state: string) => {
    /*
        Control playout (start, stop, restart)
    */
    const id = configStore.channels[configStore.i]?.id
    await authFetch(`/api/control/${id}/process`, {
        method: 'POST',
        headers: { ...configStore.contentType, ...authStore.authHeader },
        body: JSON.stringify({ command: state }),
    })
        .then(() => {
            if (state === 'start') {
                playlistStore.scrollToItem = true
            }
        })
        .catch((e) => {
            indexStore.msgAlert('error', e.data, 3)
        })
}, 2000)

const controlPlayout = throttle(async (state: string) => {
    /*
        Control playout:
        - jump to next clip
        - jump to last clip
        - reset playout state
    */
    if (playlistStore.ingestRuns) return
    const id = configStore.channels[configStore.i]?.id

    await authFetch<{ operation?: string; reason?: string }>(`/api/control/${id}/playout`, {
        method: 'POST',
        headers: { ...configStore.contentType, ...authStore.authHeader },
        body: JSON.stringify({ control: state }),
    })
        .then((result) => {
            if (result.reason === 'live_ingest_active') {
                indexStore.msgAlert('warning', t('control.navigationDisabledLive'), 3)
            }
        })
        .catch((e) => {
            indexStore.msgAlert('error', e.data, 3)
        })
}, 1000)

type Action = 'start' | 'stop' | 'restart' | 'next' | 'back' | 'reset'

const asking = ref<Action | null>(null)
const canManage = computed(() => authStore.role !== 'user')
const isRunning = computed(() => live.state === 'on_air' || live.state === 'live')
const currentTitle = computed(() => {
    const current = playlistStore.current

    return displayTitle(current.title) || filename(current.source) || t('player2.filler')
})
const nextItem = computed(() => playlistStore.playlist[playlistStore.currentIndex + 1])
const nextTitle = computed(() => {
    const item = nextItem.value

    return item ? displayTitle(item.title) || filename(item.source) : ''
})
const pillKind = computed(
    () => ({ on_air: 'ok', live: 'live', restarting: 'warn', stopped: 'bad', offline: 'idle' })[live.state] as never,
)

/** Starting is harmless; everything that interrupts what is on the air asks first. */
function ask(action: Action) {
    if (action === 'start') {
        void controlProcess('start')
    } else {
        asking.value = action
    }
}

function answer(confirmed: boolean) {
    const action = asking.value

    asking.value = null

    if (!confirmed || !action) {
        return
    }

    if (action === 'stop' || action === 'restart') {
        void controlProcess(action)
    } else {
        void controlPlayout(action)
    }
}
</script>
<template>
    <div class="w-full p-3">
        <div class="grid grid-cols-1 gap-3 lg:grid-cols-[minmax(320px,540px)_1fr]">
            <!-- Imagem do canal -->
            <section class="card bg-base-100 shadow p-3 min-w-0">
                <div class="flex items-center gap-2 mb-2">
                    <h2 class="font-bold grow">{{ t('player2.preview') }}</h2>
                    <StatusPill :kind="pillKind" :label="t(`live.${live.state}`)" />
                </div>
                <div class="w-full aspect-video bg-black rounded-lg overflow-hidden">
                    <video v-if="streamExtension === 'flv'" ref="httpStreamFlv" class="w-full h-full" controls />
                    <VideoPlayer
                        v-else-if="configStore.showPlayer && configStore.channels[configStore.i]"
                        :key="configStore.i"
                        class="live-player"
                        reference="httpStream"
                        :options="{
                            liveui: true,
                            controls: true,
                            suppressNotSupportedError: true,
                            autoplay: false,
                            preload: 'auto',
                            sources: [
                                {
                                    type: 'application/x-mpegURL',
                                    src: configStore.channels[configStore.i]?.preview_url,
                                },
                            ],
                        }"
                    />
                </div>
                <p class="mt-2 text-xs opacity-60">{{ t('player2.previewHelp') }}</p>
            </section>

            <!-- Agora no ar e controles -->
            <section class="card bg-base-100 shadow p-4 flex flex-col gap-4 min-w-0">
                <div class="flex items-start gap-3">
                    <div class="min-w-0 grow">
                        <div class="text-sm uppercase tracking-wide opacity-60">{{ t('player2.nowPlaying') }}</div>
                        <div
                            v-if="playlistStore.ingestRuns"
                            class="text-2xl font-bold truncate"
                            :title="activeIngestLabel"
                        >
                            <i class="bi bi-broadcast text-purple-500" /> {{ activeIngestLabel }}
                        </div>
                        <div
                            v-else
                            class="text-2xl font-bold truncate"
                            :class="!isRunning && 'opacity-50'"
                            :title="currentTitle"
                        >
                            {{ isRunning ? currentTitle : t('player2.nothing') }}
                        </div>
                    </div>
                    <div class="text-right shrink-0">
                        <div class="text-xs uppercase tracking-wide opacity-60">{{ t('player2.clock') }}</div>
                        <div class="text-2xl font-['DigitalNumbers'] tracking-tighter">{{ timeStr }}</div>
                    </div>
                </div>

                <div>
                    <progress
                        class="progress progress-primary w-full h-3"
                        :value="
                            playlistStore.progressValue && playlistStore.progressValue <= 100
                                ? playlistStore.progressValue
                                : 0
                        "
                        max="100"
                    />
                    <div class="mt-1 flex flex-wrap justify-between gap-x-4 text-sm opacity-80">
                        <span>{{ t('player2.left', { time: secToHMS(timeRemaining()) }) }}</span>
                        <span v-if="nextTitle" class="truncate">{{ t('player2.next', { title: nextTitle }) }}</span>
                        <span v-if="Math.abs(playlistStore.shift) > 2" class="text-warning">
                            <i class="bi bi-clock-history" />
                            {{ t('player2.shift', { s: Math.round(playlistStore.shift) }) }}
                        </span>
                    </div>
                </div>

                <!-- Botões grandes, com texto -->
                <div class="grid grid-cols-2 gap-2">
                    <button
                        v-if="!isRunning"
                        class="btn btn-primary btn-lg col-span-2 h-auto min-h-14 py-2"
                        @click="ask('start')"
                    >
                        <i class="bi bi-play-fill text-3xl" /> {{ t('player2.start') }}
                    </button>
                    <template v-else>
                        <button
                            class="btn btn-primary btn-lg h-auto min-h-14 whitespace-normal py-2 leading-tight"
                            :disabled="playlistStore.ingestRuns"
                            :title="playlistStore.ingestRuns ? t('control.navigationDisabledLive') : undefined"
                            @click="ask('next')"
                        >
                            <i class="bi bi-skip-forward-fill text-2xl" /> {{ t('player2.next_btn') }}
                        </button>
                        <button
                            class="btn btn-lg h-auto min-h-14 whitespace-normal py-2 leading-tight"
                            :disabled="playlistStore.ingestRuns"
                            :title="playlistStore.ingestRuns ? t('control.navigationDisabledLive') : undefined"
                            @click="ask('back')"
                        >
                            <i class="bi bi-skip-backward-fill text-2xl" /> {{ t('player2.back_btn') }}
                        </button>
                    </template>
                    <template v-if="canManage">
                        <button
                            class="btn h-auto min-h-12 whitespace-normal py-2 leading-tight"
                            @click="ask('restart')"
                        >
                            <i class="bi bi-arrow-clockwise" /> {{ t('player2.restart') }}
                        </button>
                        <button
                            class="btn btn-error btn-outline h-auto min-h-12 whitespace-normal py-2 leading-tight"
                            @click="ask('stop')"
                        >
                            <i class="bi bi-stop-fill" /> {{ t('player2.stop') }}
                        </button>
                    </template>
                </div>

                <!-- Volume e nível do áudio -->
                <div>
                    <div class="flex items-center gap-3">
                        <button class="btn btn-sm btn-circle text-xl" :title="t('player2.mute')" @click="muteAudio">
                            <i :class="volumeIcon()" />
                        </button>
                        <div class="grow">
                            <div class="text-sm opacity-70">{{ t('player2.volume') }}</div>
                            <input
                                v-model.number="volumeLevel"
                                type="range"
                                min="0"
                                max="1.5"
                                step="0.01"
                                class="range range-primary range-sm w-full"
                                @input="applyVolumeControl"
                            />
                        </div>
                        <span class="w-12 text-right tabular-nums">{{ Math.round(volumeLevel * 100) }}%</span>
                    </div>
                    <div class="mt-2">
                        <div class="text-xs opacity-60 mb-1">{{ t('player2.level') }}</div>
                        <div
                            class="relative h-2 w-full rounded-full overflow-hidden bg-linear-to-r from-success from-70% via-warning via-85% to-error to-100%"
                        >
                            <div
                                class="absolute top-0 right-0 h-full bg-base-300 transition-[width] duration-300"
                                :style="{ width: `${100 - audioMeterValue()}%` }"
                            />
                        </div>
                    </div>
                </div>

                <details v-if="canManage" class="text-sm">
                    <summary class="flex min-h-10 cursor-pointer items-center opacity-70">
                        {{ t('player2.more') }}
                    </summary>
                    <div class="mt-2 flex flex-wrap items-center gap-2">
                        <button class="btn btn-sm" :disabled="playlistStore.ingestRuns" @click="ask('reset')">
                            <i class="bi bi-arrow-repeat" /> {{ t('player2.reset') }}
                        </button>
                        <span class="opacity-70">{{ t('player2.resetHelp') }}</span>
                    </div>
                </details>
            </section>
        </div>

        <ConfirmModal
            :show="asking !== null"
            :danger="asking !== 'back' && asking !== 'next'"
            :title="asking ? t(`player2.confirm.${asking}.title`) : ''"
            :text="asking ? t(`player2.confirm.${asking}.text`) : ''"
            :confirm-label="asking ? t(`player2.confirm.${asking}.yes`) : ''"
            @confirm="answer(true)"
            @cancel="answer(false)"
        />
    </div>
</template>
