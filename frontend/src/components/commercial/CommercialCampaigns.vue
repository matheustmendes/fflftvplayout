<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import CampaignWizard from '@/components/commercial/CampaignWizard.vue'
import GenericModal from '@/components/utils/GenericModal.vue'

import { friendlyError } from '@/composables/friendlyError'
import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'
import { addDays, shortDate, todayInChannel, useCommercialApi } from '@/composables/commercialApi'

const SOON_DAYS = 7

const { t } = useI18n()
const api = useCommercialApi()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const canManage = computed(() => authStore.role !== 'user')
const advertisers = ref<CommercialAdvertiser[]>([])
const campaigns = ref<CommercialCampaign[]>([])
const spots = ref<CommercialSpot[]>([])
const showWizard = ref(false)
const editing = ref<CommercialCampaign | null>(null)
const archiving = ref<CommercialCampaign | null>(null)
const showAdvertisers = ref(false)
const advertiserForm = ref<Partial<CommercialAdvertiser>>({})
const archivingAdvertiser = ref<CommercialAdvertiser | null>(null)

const today = computed(() => todayInChannel(configStore.timezone || 'UTC'))
const advertiserName = (id: number) => advertisers.value.find((item) => item.id === id)?.name ?? ''
const readySpots = (campaign: CommercialCampaign) =>
    spots.value.filter((spot) => spot.campaign_id === campaign.id && spot.status === 'ready').length

function status(campaign: CommercialCampaign): { label: string; css: string } {
    if (campaign.kind === 'campaign' && campaign.end_date && campaign.end_date < today.value) {
        return { label: t('commercial.campaigns.ended'), css: 'badge-ghost' }
    }

    if (campaign.kind === 'campaign' && campaign.start_date && campaign.start_date > today.value) {
        return { label: t('commercial.campaigns.scheduled'), css: 'badge-info text-white' }
    }

    if (readySpots(campaign) === 0) {
        return { label: t('commercial.campaigns.noSpots'), css: 'badge-warning' }
    }

    if (campaign.kind === 'campaign' && campaign.end_date && campaign.end_date <= addDays(today.value, SOON_DAYS)) {
        return { label: t('commercial.campaigns.endingSoon'), css: 'badge-warning' }
    }

    return { label: t('commercial.campaigns.running'), css: 'badge-success text-white' }
}

function fail(error: unknown) {
    indexStore.msgAlert('error', friendlyError(error), 6)
}

async function load() {
    try {
        ;[advertisers.value, campaigns.value, spots.value] = await Promise.all([
            api.advertisers(),
            api.campaigns(),
            api.spots(),
        ])
    } catch (error) {
        fail(error)
    }
}

function openWizard(campaign: CommercialCampaign | null) {
    editing.value = campaign
    showWizard.value = true
}

async function saveCampaign(payload: { campaign: Partial<CommercialCampaign>; newAdvertiser: string }) {
    try {
        if (payload.newAdvertiser) {
            const created = await api.saveAdvertiser({ name: payload.newAdvertiser, active: true })

            payload.campaign.advertiser_id = created.id
        }

        await api.saveCampaign(payload.campaign)
        showWizard.value = false
        indexStore.msgAlert('success', t('commercial.saved'), 3)
        await load()
    } catch (error) {
        fail(error)
    }
}

async function archiveCampaign(confirmed: boolean) {
    const campaign = archiving.value

    archiving.value = null

    if (!confirmed || !campaign) {
        return
    }

    try {
        await api.archiveCampaign(campaign.id)
        await load()
    } catch (error) {
        fail(error)
    }
}

function editAdvertiser(advertiser: CommercialAdvertiser | null) {
    advertiserForm.value = advertiser
        ? { ...advertiser }
        : { name: '', contact_name: '', email: '', notes: '', active: true }
}

async function saveAdvertiser() {
    try {
        await api.saveAdvertiser(advertiserForm.value)
        advertiserForm.value = {}
        indexStore.msgAlert('success', t('commercial.saved'), 3)
        await load()
    } catch (error) {
        fail(error)
    }
}

async function archiveAdvertiser(confirmed: boolean) {
    const advertiser = archivingAdvertiser.value

    archivingAdvertiser.value = null

    if (!confirmed || !advertiser) {
        return
    }

    try {
        await api.archiveAdvertiser(advertiser.id)
        await load()
    } catch (error) {
        fail(error)
    }
}

onMounted(load)
watch(() => configStore.i, load)
</script>
<template>
    <div class="space-y-5">
        <div class="flex items-center gap-2">
            <h2 class="font-bold text-lg grow">{{ t('commercial.tab.campaigns') }}</h2>
            <button v-if="canManage" class="btn btn-sm btn-accent" @click="openWizard(null)">
                <i class="bi bi-plus-lg" /> {{ t('commercial.campaigns.new') }}
            </button>
        </div>

        <p v-if="!campaigns.length" class="opacity-70">{{ t('commercial.campaigns.empty') }}</p>
        <div v-else class="grid gap-3 md:grid-cols-2">
            <div v-for="campaign in campaigns" :key="campaign.id" class="card bg-base-100 shadow p-4">
                <div class="flex items-start gap-2">
                    <div class="grow min-w-0">
                        <div class="font-semibold truncate">{{ campaign.name }}</div>
                        <div class="text-sm opacity-70 truncate">{{ advertiserName(campaign.advertiser_id) }}</div>
                    </div>
                    <span class="badge badge-sm shrink-0" :class="status(campaign).css">{{
                        status(campaign).label
                    }}</span>
                </div>
                <dl class="mt-2 text-sm grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5">
                    <dt class="opacity-60">{{ t('commercial.campaigns.period') }}</dt>
                    <dd>
                        <template v-if="campaign.kind === 'campaign'">
                            {{ shortDate(campaign.start_date) }} → {{ shortDate(campaign.end_date) }}
                        </template>
                        <template v-else>{{ t('commercial.campaigns.noDeadline') }}</template>
                    </dd>
                    <dt class="opacity-60">{{ t('commercial.wizard.perDay') }}</dt>
                    <dd>{{ campaign.max_per_day ?? t('commercial.wizard.noLimit') }}</dd>
                    <dt class="opacity-60">{{ t('commercial.wizard.position') }}</dt>
                    <dd>{{ t(`commercial.position.${campaign.position}`) }}</dd>
                    <dt class="opacity-60">{{ t('commercial.campaigns.readySpots') }}</dt>
                    <dd>{{ readySpots(campaign) }}</dd>
                </dl>
                <div v-if="canManage" class="mt-3 flex justify-end gap-1">
                    <button class="btn btn-xs" @click="openWizard(campaign)">{{ t('commercial.edit') }}</button>
                    <button class="btn btn-xs btn-ghost" :title="t('commercial.remove')" @click="archiving = campaign">
                        <i class="bi bi-archive" />
                    </button>
                </div>
            </div>
        </div>

        <section class="card bg-base-100 shadow p-4">
            <button class="flex items-center gap-2 font-bold text-left" @click="showAdvertisers = !showAdvertisers">
                <i class="bi" :class="showAdvertisers ? 'bi-chevron-down' : 'bi-chevron-right'" />
                {{ t('commercial.advertisers.title') }} ({{ advertisers.length }})
            </button>
            <div v-if="showAdvertisers" class="mt-3 space-y-3">
                <div v-for="advertiser in advertisers" :key="advertiser.id" class="flex items-center gap-2 text-sm">
                    <div class="grow min-w-0">
                        <div class="font-medium truncate">{{ advertiser.name }}</div>
                        <div class="opacity-60 truncate">{{ advertiser.contact_name }} {{ advertiser.email }}</div>
                    </div>
                    <template v-if="canManage">
                        <button class="btn btn-xs" @click="editAdvertiser(advertiser)">
                            {{ t('commercial.edit') }}
                        </button>
                        <button
                            class="btn btn-xs btn-ghost"
                            :title="t('commercial.remove')"
                            @click="archivingAdvertiser = advertiser"
                        >
                            <i class="bi bi-archive" />
                        </button>
                    </template>
                </div>
                <button
                    v-if="canManage && advertiserForm.name === undefined"
                    class="btn btn-sm"
                    @click="editAdvertiser(null)"
                >
                    <i class="bi bi-plus-lg" /> {{ t('commercial.advertisers.new') }}
                </button>
                <div v-if="advertiserForm.name !== undefined" class="grid gap-2 sm:grid-cols-3">
                    <input
                        v-model="advertiserForm.name"
                        class="input input-sm"
                        maxlength="100"
                        :placeholder="t('commercial.advertisers.name')"
                    />
                    <input
                        v-model="advertiserForm.contact_name"
                        class="input input-sm"
                        :placeholder="t('commercial.advertisers.contact')"
                    />
                    <input
                        v-model="advertiserForm.email"
                        type="email"
                        class="input input-sm"
                        :placeholder="t('commercial.advertisers.email')"
                    />
                    <div class="sm:col-span-3 flex gap-2 justify-end">
                        <button class="btn btn-sm" @click="advertiserForm = {}">{{ t('cancel') }}</button>
                        <button
                            class="btn btn-sm btn-accent"
                            :disabled="!advertiserForm.name?.trim()"
                            @click="saveAdvertiser"
                        >
                            {{ t('commercial.save') }}
                        </button>
                    </div>
                </div>
            </div>
        </section>

        <CampaignWizard
            :show="showWizard"
            :advertisers="advertisers"
            :campaign="editing"
            @close="showWizard = false"
            @save="saveCampaign"
        />
        <GenericModal
            :show="archiving !== null"
            :title="t('commercial.campaigns.archiveTitle')"
            :text="t('commercial.campaigns.archiveText', { name: archiving?.name ?? '' })"
            :modal-action="archiveCampaign"
        />
        <GenericModal
            :show="archivingAdvertiser !== null"
            :title="t('commercial.advertisers.archiveTitle')"
            :text="t('commercial.advertisers.archiveText', { name: archivingAdvertiser?.name ?? '' })"
            :modal-action="archiveAdvertiser"
        />
    </div>
</template>
