<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useHead } from '@unhead/vue'

import SettingsCard from '@/components/config/SettingsCard.vue'
import ConfirmModal from '@/components/ui/ConfirmModal.vue'
import EmptyState from '@/components/ui/EmptyState.vue'
import PageHeader from '@/components/ui/PageHeader.vue'
import StatusPill from '@/components/ui/StatusPill.vue'
import type { StatusKind } from '@/components/ui/StatusPill.vue'

import { formatDuration } from '@/composables/commercialApi'
import { friendlyError } from '@/composables/friendlyError'
import {
    YOUTUBE_HEIGHTS,
    youtubeApi,
    type YoutubeJob,
    type YoutubePreview,
    type YoutubeSource,
    type YoutubeView,
} from '@/composables/youtubeApi'
import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'

const POLL_RUNNING_MS = 2000
const PREVIEW_VIDEOS = 8
/** Full HD H.264 from YouTube is about 8 Mbps with the sound: 1 MB per second. */
const MEGABYTES_PER_SECOND = { 1080: 1, 720: 0.55, 480: 0.3 } as const

const { t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

useHead({ title: computed(() => t('nav.youtube')) })

const channel = computed(() => configStore.channels[configStore.i]?.id)
const isGlobalAdmin = computed(() => authStore.role === 'global_admin')

const view = ref<YoutubeView | null>(null)
const loaded = ref(false)

const sourceUrl = ref('')
const addingSource = ref(false)
const removing = ref<YoutubeSource | null>(null)

const importUrl = ref('')
const looking = ref(false)
const preview = ref<YoutubePreview | null>(null)
const previewedUrl = ref('')
const folder = ref('')
const height = ref<number>(1080)
const starting = ref(false)

const stopping = ref<YoutubeJob | null>(null)

let timer: ReturnType<typeof setTimeout> | undefined

const hasRunningJob = computed(() => view.value?.jobs.some((job) => job.status === 'running') ?? false)
const installed = computed(() => view.value?.version != null)

const totalSeconds = computed(() => preview.value?.videos.reduce((sum, video) => sum + (video.duration ?? 0), 0) ?? 0)
const hoursText = computed(() => formatDuration(totalSeconds.value))
const spaceText = computed(() => {
    const megabytes = totalSeconds.value * (MEGABYTES_PER_SECOND[height.value as 1080 | 720 | 480] ?? 1)

    return megabytes >= 1000 ? `${(megabytes / 1000).toFixed(1)} GB` : `${Math.round(megabytes)} MB`
})
const canStart = computed(
    () =>
        installed.value &&
        preview.value?.allowed === true &&
        importUrl.value.trim() === previewedUrl.value &&
        !starting.value,
)

function fail(error: unknown) {
    indexStore.msgAlert('error', friendlyError(error), 8)
}

function schedule() {
    clearTimeout(timer)

    if (hasRunningJob.value) {
        timer = setTimeout(refreshJobs, POLL_RUNNING_MS)
    }
}

async function refreshJobs() {
    if (channel.value === undefined) {
        return
    }

    try {
        const jobs = await youtubeApi.jobs(channel.value)

        if (view.value) {
            view.value = { ...view.value, jobs }
        }
    } catch {
        // The next round tries again.
    }

    schedule()
}

async function load() {
    if (channel.value === undefined) {
        return
    }

    try {
        view.value = await youtubeApi.get(channel.value)
    } catch (error) {
        fail(error)
    } finally {
        loaded.value = true
    }

    schedule()
}

async function addSource() {
    if (channel.value === undefined || !sourceUrl.value.trim() || addingSource.value) {
        return
    }

    addingSource.value = true

    try {
        const sources = await youtubeApi.addSource(channel.value, sourceUrl.value.trim())

        view.value = view.value ? { ...view.value, sources } : view.value
        sourceUrl.value = ''
        indexStore.msgAlert('success', t('youtube.sources.added'), 3)
    } catch (error) {
        fail(error)
    } finally {
        addingSource.value = false
    }
}

async function removeSource() {
    const source = removing.value

    removing.value = null

    if (!source || channel.value === undefined) {
        return
    }

    try {
        const sources = await youtubeApi.removeSource(channel.value, source.id)

        view.value = view.value ? { ...view.value, sources } : view.value
        indexStore.msgAlert('success', t('youtube.sources.removed'), 3)
    } catch (error) {
        fail(error)
    }
}

async function look() {
    const url = importUrl.value.trim()

    if (channel.value === undefined || !url || looking.value) {
        return
    }

    looking.value = true
    preview.value = null

    try {
        preview.value = await youtubeApi.preview(channel.value, url)
        previewedUrl.value = url
        folder.value = preview.value.default_folder
    } catch (error) {
        fail(error)
    } finally {
        looking.value = false
    }
}

async function start() {
    if (channel.value === undefined || !canStart.value) {
        return
    }

    starting.value = true

    try {
        const job = await youtubeApi.start(channel.value, previewedUrl.value, folder.value.trim(), height.value)

        if (view.value) {
            view.value = { ...view.value, jobs: [job, ...view.value.jobs] }
        }

        indexStore.msgAlert('success', t('youtube.import.started'), 4)
        schedule()
    } catch (error) {
        fail(error)
    } finally {
        starting.value = false
    }
}

async function stop() {
    const job = stopping.value

    stopping.value = null

    if (!job || channel.value === undefined) {
        return
    }

    try {
        await youtubeApi.cancel(channel.value, job.id)
        indexStore.msgAlert('success', t('youtube.jobs.stopped'), 3)
        await refreshJobs()
    } catch (error) {
        fail(error)
    }
}

function jobKind(job: YoutubeJob): StatusKind {
    return { running: 'live', done: 'ok', failed: 'bad', cancelled: 'idle' }[job.status] as StatusKind
}

watch(channel, load)
onMounted(load)
onBeforeUnmount(() => clearTimeout(timer))
</script>
<template>
    <div class="mx-auto w-full max-w-4xl space-y-5 p-4 pb-8">
        <PageHeader :title="t('youtube.title')" :subtitle="t('youtube.subtitle')" />

        <div v-if="loaded && !installed" class="alert alert-warning items-start" role="alert">
            <i class="bi bi-exclamation-triangle text-xl" />
            <div>
                <p class="font-semibold">{{ t('youtube.notInstalled') }}</p>
                <p class="text-sm">{{ t('youtube.notInstalledHelp') }}</p>
            </div>
        </div>
        <p v-else-if="view" class="text-sm opacity-60">
            <i class="bi bi-youtube" /> {{ t('youtube.version', { version: view.version }) }}
            <span v-if="view.cookies"> · {{ t('youtube.cookiesOn') }}</span>
        </p>

        <SettingsCard :title="t('youtube.sources.title')" :text="t('youtube.sources.text')" icon="bi-shield-check">
            <ul v-if="view?.sources.length" class="divide-y divide-base-300">
                <li v-for="source in view.sources" :key="source.id" class="flex min-h-12 items-center gap-3 py-2">
                    <i class="bi bi-youtube text-xl text-error" />
                    <div class="min-w-0 grow">
                        <div class="truncate font-semibold">{{ source.name || source.youtube_channel_id }}</div>
                        <div class="truncate text-xs opacity-50">{{ source.youtube_channel_id }}</div>
                    </div>
                    <button
                        v-if="isGlobalAdmin"
                        type="button"
                        class="btn btn-sm btn-ghost btn-circle"
                        :title="t('youtube.sources.remove')"
                        :aria-label="t('youtube.sources.remove')"
                        @click="removing = source"
                    >
                        <i class="bi bi-trash" />
                    </button>
                </li>
            </ul>
            <EmptyState
                v-else-if="loaded"
                icon="bi-shield-exclamation"
                :title="t('youtube.sources.empty')"
                :text="isGlobalAdmin ? t('youtube.sources.emptyText') : t('youtube.sources.emptyAdmin')"
            />

            <form v-if="isGlobalAdmin" class="flex flex-col gap-2 sm:flex-row" @submit.prevent="addSource">
                <input
                    v-model="sourceUrl"
                    type="url"
                    inputmode="url"
                    class="input input-sm w-full"
                    :placeholder="t('youtube.sources.placeholder')"
                />
                <button class="btn btn-sm btn-primary" :disabled="!sourceUrl.trim() || addingSource || !installed">
                    <span v-if="addingSource" class="loading loading-spinner loading-xs" />
                    <i v-else class="bi bi-plus-lg" />
                    {{ t('youtube.sources.add') }}
                </button>
            </form>
        </SettingsCard>

        <SettingsCard :title="t('youtube.import.title')" :text="t('youtube.import.text')" icon="bi-cloud-download">
            <form class="flex flex-col gap-2 sm:flex-row" @submit.prevent="look">
                <input
                    v-model="importUrl"
                    type="url"
                    inputmode="url"
                    class="input input-sm w-full"
                    :aria-label="t('youtube.import.url')"
                    :placeholder="t('youtube.import.urlPlaceholder')"
                />
                <button class="btn btn-sm btn-primary" :disabled="!importUrl.trim() || looking || !installed">
                    <span v-if="looking" class="loading loading-spinner loading-xs" />
                    <i v-else class="bi bi-search" />
                    {{ looking ? t('youtube.import.looking') : t('youtube.import.look') }}
                </button>
            </form>

            <div v-if="preview" class="space-y-4 rounded-xl border border-base-300 p-4">
                <div class="flex flex-wrap items-start gap-2">
                    <div class="min-w-0 grow">
                        <div class="text-sm opacity-60">{{ t(`youtube.import.kind.${preview.kind}`) }}</div>
                        <div class="break-words text-lg font-bold">{{ preview.title }}</div>
                        <div class="text-sm opacity-70">
                            {{ t('youtube.import.owner', { name: preview.owner_name }) }}
                        </div>
                    </div>
                    <StatusPill
                        :kind="preview.allowed ? 'ok' : 'bad'"
                        :label="preview.allowed ? t('youtube.import.allowed') : t('youtube.import.notAllowed')"
                    />
                </div>

                <div v-if="!preview.allowed" class="alert alert-error text-sm" role="alert">
                    <i class="bi bi-slash-circle" /> {{ t('youtube.import.notAllowedText') }}
                </div>

                <template v-else>
                    <p class="text-sm opacity-80">
                        <template v-if="preview.kind === 'collection'">
                            {{
                                t(
                                    'youtube.import.playlists',
                                    { count: preview.playlists.length },
                                    preview.playlists.length,
                                )
                            }}
                        </template>
                        <template v-else>
                            {{ t('youtube.import.videos', { count: preview.videos.length }, preview.videos.length) }}
                            <template v-if="totalSeconds > 0">
                                · {{ t('youtube.import.hours', { time: hoursText }) }} ·
                                {{ t('youtube.import.space', { size: spaceText }) }}
                            </template>
                        </template>
                    </p>

                    <ul class="space-y-1 text-sm">
                        <template v-if="preview.kind === 'collection'">
                            <li
                                v-for="playlist in preview.playlists.slice(0, PREVIEW_VIDEOS)"
                                :key="playlist.url"
                                class="truncate"
                            >
                                <i class="bi bi-collection-play opacity-50" /> {{ playlist.title }}
                            </li>
                            <li v-if="preview.playlists.length > PREVIEW_VIDEOS" class="opacity-60">
                                {{ t('youtube.import.more', { count: preview.playlists.length - PREVIEW_VIDEOS }) }}
                            </li>
                        </template>
                        <template v-else>
                            <li
                                v-for="video in preview.videos.slice(0, PREVIEW_VIDEOS)"
                                :key="video.id"
                                class="flex gap-2"
                            >
                                <i class="bi bi-play-btn opacity-50" />
                                <span class="min-w-0 grow truncate">{{ video.title }}</span>
                                <span class="shrink-0 opacity-60">{{ formatDuration(video.duration) }}</span>
                            </li>
                            <li v-if="preview.videos.length > PREVIEW_VIDEOS" class="opacity-60">
                                {{ t('youtube.import.more', { count: preview.videos.length - PREVIEW_VIDEOS }) }}
                            </li>
                        </template>
                    </ul>

                    <div class="grid gap-3 sm:grid-cols-[1fr_10rem]">
                        <label class="form-control">
                            <span class="label-text mb-1">{{ t('youtube.import.folder') }}</span>
                            <input v-model="folder" class="input input-sm w-full" maxlength="200" />
                            <span class="mt-1 text-xs opacity-60">{{ t('youtube.import.folderHelp') }}</span>
                        </label>
                        <label class="form-control">
                            <span class="label-text mb-1">{{ t('youtube.import.quality') }}</span>
                            <select v-model.number="height" class="select select-sm w-full">
                                <option v-for="option in YOUTUBE_HEIGHTS" :key="option" :value="option">
                                    {{ option }}p
                                </option>
                            </select>
                            <span class="mt-1 text-xs opacity-60">{{ t('youtube.import.qualityHelp') }}</span>
                        </label>
                    </div>

                    <button type="button" class="btn btn-primary w-full sm:w-auto" :disabled="!canStart" @click="start">
                        <span v-if="starting" class="loading loading-spinner loading-xs" />
                        <i v-else class="bi bi-cloud-download" />
                        {{ t('youtube.import.start') }}
                    </button>
                </template>
            </div>
        </SettingsCard>

        <SettingsCard :title="t('youtube.jobs.title')" icon="bi-list-task">
            <EmptyState v-if="loaded && !view?.jobs.length" icon="bi-inbox" :title="t('youtube.jobs.empty')" />

            <ul v-else class="space-y-3">
                <li v-for="job in view?.jobs" :key="job.id" class="space-y-2 rounded-xl border border-base-300 p-3">
                    <div class="flex flex-wrap items-start gap-2">
                        <div class="min-w-0 grow">
                            <div class="break-words font-semibold">{{ job.title }}</div>
                            <div class="truncate text-xs opacity-60">
                                {{ t('youtube.jobs.folder', { folder: job.folder }) }}
                            </div>
                        </div>
                        <StatusPill :kind="jobKind(job)" :label="t(`youtube.jobs.status.${job.status}`)" />
                        <button
                            v-if="job.status === 'running'"
                            type="button"
                            class="btn btn-sm btn-outline btn-error"
                            @click="stopping = job"
                        >
                            <i class="bi bi-stop-circle" /> {{ t('youtube.jobs.stop') }}
                        </button>
                    </div>

                    <div v-if="job.status === 'running'" class="space-y-1">
                        <progress class="progress progress-primary w-full" :value="job.percent" max="100" />
                        <div class="flex flex-wrap gap-x-3 text-xs opacity-70">
                            <span v-if="job.playlist_total > 1">
                                {{
                                    t('youtube.jobs.playlistOf', {
                                        index: job.playlist_index,
                                        total: job.playlist_total,
                                    })
                                }}
                            </span>
                            <span v-if="job.item_total > 0">
                                {{ t('youtube.jobs.itemOf', { index: job.item_index, total: job.item_total }) }}
                            </span>
                            <span>{{ Math.round(job.percent) }}%</span>
                        </div>
                        <div v-if="job.current" class="truncate text-sm">{{ job.current }}</div>
                    </div>

                    <div class="flex flex-wrap gap-x-4 gap-y-1 text-sm">
                        <span class="text-success"
                            ><i class="bi bi-check2-circle" />
                            {{ t('youtube.jobs.downloaded', { count: job.downloaded }) }}</span
                        >
                        <span v-if="job.already_had" class="opacity-70">
                            <i class="bi bi-files" /> {{ t('youtube.jobs.alreadyHad', { count: job.already_had }) }}
                        </span>
                        <span v-if="job.rejected" class="text-warning">
                            <i class="bi bi-slash-circle" /> {{ t('youtube.jobs.rejected', { count: job.rejected }) }}
                        </span>
                        <span v-if="job.failed" class="text-error">
                            <i class="bi bi-x-circle" /> {{ t('youtube.jobs.failed', { count: job.failed }) }}
                        </span>
                    </div>

                    <ul v-if="job.errors.length" class="space-y-1 text-sm text-error">
                        <li v-for="message in job.errors" :key="message">{{ message }}</li>
                    </ul>

                    <RouterLink
                        v-if="job.status !== 'running' && job.downloaded > 0"
                        :to="{ path: '/media', query: { channel } }"
                        class="btn btn-xs btn-link px-0"
                    >
                        <i class="bi bi-folder2-open" /> {{ t('youtube.jobs.seeFiles') }}
                    </RouterLink>
                </li>
            </ul>
        </SettingsCard>

        <ConfirmModal
            :show="removing !== null"
            danger
            :title="t('youtube.sources.removeTitle')"
            :text="t('youtube.sources.removeText', { name: removing?.name ?? '' })"
            :confirm-label="t('youtube.sources.remove')"
            @confirm="removeSource"
            @cancel="removing = null"
        />
        <ConfirmModal
            :show="stopping !== null"
            danger
            :title="t('youtube.jobs.stopTitle')"
            :text="t('youtube.jobs.stopText')"
            :confirm-label="t('youtube.jobs.stop')"
            @confirm="stop"
            @cancel="stopping = null"
        />
    </div>
</template>
