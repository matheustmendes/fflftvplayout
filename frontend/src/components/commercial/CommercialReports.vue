<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'
import { addDays, shortDate, todayInChannel, useCommercialApi } from '@/composables/commercialApi'

const REPORT_DEFAULT_DAYS = 30

const { t } = useI18n()
const api = useCommercialApi()
const configStore = useConfig()
const indexStore = useIndex()

const advertisers = ref<CommercialAdvertiser[]>([])
const campaigns = ref<CommercialCampaign[]>([])
const advertiserId = ref<number | null>(null)
const campaignId = ref<number | null>(null)
const today = todayInChannel(configStore.timezone || 'UTC')
const dateFrom = ref(addDays(today, -REPORT_DEFAULT_DAYS))
const dateTo = ref(today)
const report = ref<CommercialReport | null>(null)
const busy = ref(false)

const campaignOptions = computed(() =>
    campaigns.value.filter((campaign) => advertiserId.value === null || campaign.advertiser_id === advertiserId.value),
)
const periodError = computed(() => {
    if (!dateFrom.value || !dateTo.value) {
        return t('commercial.reports.needPeriod')
    }

    return dateFrom.value > dateTo.value ? t('commercial.wizard.datesOrder') : ''
})

function query() {
    const params = new URLSearchParams({ date_from: dateFrom.value, date_to: dateTo.value })

    if (campaignId.value !== null) {
        params.set('campaign_id', String(campaignId.value))
    } else if (advertiserId.value !== null) {
        params.set('advertiser_id', String(advertiserId.value))
    }

    return params.toString()
}

function fail(error: unknown) {
    indexStore.msgAlert('error', error instanceof Error ? error.message : String(error), 5)
}

async function load() {
    try {
        ;[advertisers.value, campaigns.value] = await Promise.all([api.advertisers(true), api.campaigns(true)])
    } catch (error) {
        fail(error)
    }
}

async function show() {
    busy.value = true

    try {
        report.value = await api.reportSummary(query())
    } catch (error) {
        report.value = null
        fail(error)
    } finally {
        busy.value = false
    }
}

async function download(kind: 'pdf' | 'csv') {
    busy.value = true

    try {
        await api.download(`/reports/export.${kind}?${query()}`, `relatorio.${kind}`)
    } catch (error) {
        fail(error)
    } finally {
        busy.value = false
    }
}

function pickCampaign() {
    const campaign = campaigns.value.find((item) => item.id === campaignId.value)

    if (campaign) {
        advertiserId.value = campaign.advertiser_id

        if (campaign.kind === 'campaign' && campaign.start_date && campaign.end_date) {
            dateFrom.value = campaign.start_date
            dateTo.value = campaign.end_date < today ? campaign.end_date : today
        }
    }
}

watch(advertiserId, () => {
    if (campaignId.value !== null && !campaignOptions.value.some((item) => item.id === campaignId.value)) {
        campaignId.value = null
    }
})
onMounted(load)
watch(() => configStore.i, load)
</script>
<template>
    <div class="space-y-5">
        <p class="text-sm opacity-80">{{ t('commercial.reports.help') }}</p>

        <section class="card bg-base-100 shadow p-4 grid gap-3 md:grid-cols-2">
            <label class="form-control">
                <span class="label-text mb-1">{{ t('commercial.reports.advertiser') }}</span>
                <select v-model="advertiserId" class="select select-sm w-full">
                    <option :value="null">{{ t('commercial.reports.all') }}</option>
                    <option v-for="advertiser in advertisers" :key="advertiser.id" :value="advertiser.id">
                        {{ advertiser.name }}
                    </option>
                </select>
            </label>
            <label class="form-control">
                <span class="label-text mb-1">{{ t('commercial.campaigns.campaign') }}</span>
                <select v-model="campaignId" class="select select-sm w-full" @change="pickCampaign">
                    <option :value="null">{{ t('commercial.reports.all') }}</option>
                    <option v-for="campaign in campaignOptions" :key="campaign.id" :value="campaign.id">
                        {{ campaign.name }}
                    </option>
                </select>
            </label>
            <label class="form-control">
                <span class="label-text mb-1">{{ t('commercial.reports.from') }}</span>
                <input v-model="dateFrom" type="date" class="input input-sm w-full" />
            </label>
            <label class="form-control">
                <span class="label-text mb-1">{{ t('commercial.reports.to') }}</span>
                <input v-model="dateTo" type="date" class="input input-sm w-full" />
            </label>
            <p v-if="periodError" class="md:col-span-2 text-error text-sm">{{ periodError }}</p>
            <div class="md:col-span-2 flex flex-wrap gap-2">
                <button class="btn btn-sm" :disabled="busy || !!periodError" @click="show">
                    <i class="bi bi-eye" /> {{ t('commercial.reports.view') }}
                </button>
                <button class="btn btn-sm btn-accent" :disabled="busy || !!periodError" @click="download('pdf')">
                    <i class="bi bi-file-earmark-pdf" /> {{ t('commercial.reports.pdf') }}
                </button>
                <button class="btn btn-sm btn-accent" :disabled="busy || !!periodError" @click="download('csv')">
                    <i class="bi bi-file-earmark-spreadsheet" /> {{ t('commercial.reports.csv') }}
                </button>
            </div>
        </section>

        <section v-if="report" class="card bg-base-100 shadow p-4 space-y-3">
            <div class="text-sm">
                <div class="font-bold text-lg">
                    {{ report.header.campaign ?? report.header.advertiser ?? t('commercial.reports.all') }}
                </div>
                {{ shortDate(report.header.date_from) }} — {{ shortDate(report.header.date_to) }}
            </div>
            <div v-for="note in report.notes" :key="note" class="alert alert-warning text-black text-sm">
                {{ note }}
            </div>
            <p v-if="!report.days.length" class="opacity-70">{{ t('commercial.reports.none') }}</p>
            <div v-else class="overflow-x-auto">
                <table class="table table-sm">
                    <thead>
                        <tr>
                            <th>{{ t('commercial.reports.day') }}</th>
                            <th>{{ t('commercial.reports.planned') }}</th>
                            <th>{{ t('commercial.reports.aired') }}</th>
                            <th>{{ t('commercial.reports.partial') }}</th>
                            <th>{{ t('commercial.reports.missed') }}</th>
                            <th>{{ t('commercial.reports.pending') }}</th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr v-for="day in report.days" :key="day.date">
                            <td>{{ shortDate(day.date) }}</td>
                            <td>{{ day.planned }}</td>
                            <td>{{ day.aired }}</td>
                            <td>{{ day.partial }}</td>
                            <td :class="day.missed > 0 && 'text-error'">{{ day.missed }}</td>
                            <td>{{ day.pending }}</td>
                        </tr>
                        <tr class="font-bold">
                            <td>{{ t('commercial.reports.total') }}</td>
                            <td>{{ report.totals.planned }}</td>
                            <td>{{ report.totals.aired }}</td>
                            <td>{{ report.totals.partial }}</td>
                            <td>{{ report.totals.missed }}</td>
                            <td>{{ report.totals.pending }}</td>
                        </tr>
                    </tbody>
                </table>
            </div>
        </section>
    </div>
</template>
