<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import GenericModal from '@/components/utils/GenericModal.vue'

import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'
import { useFolderUpload } from '@/composables/commercialUpload'
import { formatDuration, useCommercialApi } from '@/composables/commercialApi'

const SPOT_FOLDER = 'comerciais-entrada'
const ACCEPTED = '.mxf,.mp4,.mov,.mpg,.mpeg,.m4v,.avi,.mkv'

const { t } = useI18n()
const api = useCommercialApi()
const configStore = useConfig()
const indexStore = useIndex()
const { uploadToFolder, progress, uploading, reset } = useFolderUpload()

const campaigns = ref<CommercialCampaign[]>([])
const advertisers = ref<CommercialAdvertiser[]>([])
const spots = ref<CommercialSpot[]>([])
const campaignId = ref<number | null>(null)
const title = ref('')
const file = ref<File | null>(null)
const fileInput = ref<HTMLInputElement>()
const removing = ref<CommercialSpot | null>(null)
let pollTimer: ReturnType<typeof setInterval> | undefined

const advertiserName = (id: number) => advertisers.value.find((item) => item.id === id)?.name ?? ''
const campaignLabel = (campaign: CommercialCampaign) => `${advertiserName(campaign.advertiser_id)} — ${campaign.name}`
const campaignById = (id: number) => campaigns.value.find((item) => item.id === id)
const canSend = computed(() => campaignId.value !== null && title.value.trim() !== '' && file.value && !uploading.value)
const hasProcessing = computed(() => spots.value.some((spot) => spot.status === 'processing'))

function onFile(event: Event) {
    const chosen = (event.target as HTMLInputElement).files?.[0] ?? null

    file.value = chosen

    if (chosen && !title.value.trim()) {
        title.value = chosen.name.replace(/\.[^.]+$/, '')
    }
}

function fail(error: unknown) {
    indexStore.msgAlert('error', error instanceof Error ? error.message : String(error), 5)
}

async function loadSpots() {
    try {
        spots.value = await api.spots()
    } catch (error) {
        fail(error)
    }
}

async function loadAll() {
    try {
        ;[campaigns.value, advertisers.value] = await Promise.all([api.campaigns(), api.advertisers()])
    } catch (error) {
        fail(error)
    }

    await loadSpots()
}

async function send() {
    if (!canSend.value || campaignId.value === null || !file.value) {
        return
    }

    try {
        const path = await uploadToFolder(file.value, SPOT_FOLDER)

        await api.addSpot(campaignId.value, title.value.trim(), path)
        indexStore.msgAlert('success', t('commercial.spots.sent'), 4)
        title.value = ''
        file.value = null

        if (fileInput.value) {
            fileInput.value.value = ''
        }

        await loadSpots()
    } catch (error) {
        fail(error)
    } finally {
        reset()
    }
}

async function retry(spot: CommercialSpot) {
    try {
        await api.retrySpot(spot.id)
        await loadSpots()
    } catch (error) {
        fail(error)
    }
}

async function remove(confirmed: boolean) {
    const spot = removing.value

    removing.value = null

    if (!confirmed || !spot) {
        return
    }

    try {
        await api.removeSpot(spot.id)
        await loadSpots()
    } catch (error) {
        fail(error)
    }
}

onMounted(() => {
    void loadAll()
    pollTimer = setInterval(() => {
        if (hasProcessing.value) {
            void loadSpots()
        }
    }, 2000)
})

onBeforeUnmount(() => clearInterval(pollTimer))
watch(() => configStore.i, loadAll)
</script>
<template>
    <div class="space-y-5">
        <section class="card bg-base-100 shadow p-4">
            <h2 class="font-bold text-lg mb-1">{{ t('commercial.spots.new') }}</h2>
            <p class="text-sm opacity-70 mb-3">{{ t('commercial.spots.help') }}</p>
            <div v-if="!campaigns.length" class="alert alert-warning text-black">
                {{ t('commercial.spots.needCampaign') }}
            </div>
            <div v-else class="grid gap-3 md:grid-cols-2">
                <label class="form-control">
                    <span class="label-text mb-1">{{ t('commercial.campaigns.campaign') }}</span>
                    <select v-model="campaignId" class="select select-sm w-full">
                        <option :value="null" disabled>{{ t('commercial.choose') }}</option>
                        <option v-for="campaign in campaigns" :key="campaign.id" :value="campaign.id">
                            {{ campaignLabel(campaign) }}
                        </option>
                    </select>
                </label>
                <label class="form-control">
                    <span class="label-text mb-1">{{ t('commercial.spots.spotName') }}</span>
                    <input v-model="title" class="input input-sm w-full" maxlength="100" />
                </label>
                <label class="form-control md:col-span-2">
                    <span class="label-text mb-1">{{ t('commercial.spots.file') }}</span>
                    <input
                        ref="fileInput"
                        type="file"
                        :accept="ACCEPTED"
                        class="file-input file-input-sm w-full"
                        @change="onFile"
                    />
                </label>
                <div class="md:col-span-2 flex items-center gap-3">
                    <button class="btn btn-sm btn-accent" :disabled="!canSend" @click="send">
                        <i class="bi bi-upload" /> {{ t('commercial.spots.send') }}
                    </button>
                    <progress v-if="uploading" class="progress progress-accent grow" :value="progress" max="100" />
                    <span v-if="uploading" class="text-sm">{{ Math.round(progress) }}%</span>
                </div>
            </div>
        </section>

        <section class="card bg-base-100 shadow p-4">
            <h2 class="font-bold text-lg mb-2">{{ t('commercial.spots.list') }}</h2>
            <p v-if="!spots.length" class="opacity-70">{{ t('commercial.spots.empty') }}</p>
            <div v-else class="overflow-x-auto">
                <table class="table table-sm">
                    <thead>
                        <tr>
                            <th>{{ t('commercial.spots.title') }}</th>
                            <th>{{ t('commercial.campaigns.campaign') }}</th>
                            <th>{{ t('commercial.spots.duration') }}</th>
                            <th>{{ t('commercial.spots.state') }}</th>
                            <th />
                        </tr>
                    </thead>
                    <tbody>
                        <tr v-for="spot in spots" :key="spot.id">
                            <td>
                                <div class="font-medium">{{ spot.title }}</div>
                                <div class="text-xs opacity-60">{{ spot.original_filename }}</div>
                            </td>
                            <td>
                                <template v-if="campaignById(spot.campaign_id)">
                                    {{ campaignLabel(campaignById(spot.campaign_id)!) }}
                                </template>
                            </td>
                            <td>{{ formatDuration(spot.duration_sec) }}</td>
                            <td class="min-w-48">
                                <template v-if="spot.status === 'processing'">
                                    <span class="badge badge-sm badge-info text-white">{{
                                        t('commercial.spots.converting')
                                    }}</span>
                                    <progress
                                        class="progress progress-info w-full mt-1"
                                        :value="spot.progress"
                                        max="100"
                                    />
                                </template>
                                <span
                                    v-else-if="spot.status === 'ready'"
                                    class="badge badge-sm badge-success text-white"
                                >
                                    {{ t('commercial.spots.ready') }}
                                </span>
                                <template v-else>
                                    <span class="badge badge-sm badge-error text-white">{{
                                        t('commercial.spots.failed')
                                    }}</span>
                                    <div class="text-xs mt-1 text-error">{{ spot.error }}</div>
                                </template>
                            </td>
                            <td class="text-right whitespace-nowrap">
                                <button v-if="spot.status === 'failed'" class="btn btn-xs" @click="retry(spot)">
                                    {{ t('commercial.spots.retry') }}
                                </button>
                                <button
                                    class="btn btn-xs btn-ghost"
                                    :title="t('commercial.remove')"
                                    @click="removing = spot"
                                >
                                    <i class="bi bi-trash" />
                                </button>
                            </td>
                        </tr>
                    </tbody>
                </table>
            </div>
        </section>

        <GenericModal
            :show="removing !== null"
            :title="t('commercial.spots.removeTitle')"
            :text="t('commercial.spots.removeText', { name: removing?.title ?? '' })"
            :modal-action="remove"
        />
    </div>
</template>
