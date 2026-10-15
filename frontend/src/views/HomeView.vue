<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useHead } from '@unhead/vue'
import { RouterLink } from 'vue-router'

import CoverageStrip from '@/components/dashboard/CoverageStrip.vue'
import type { CoverageDay } from '@/components/dashboard/CoverageStrip.vue'
import NowOnAir from '@/components/dashboard/NowOnAir.vue'
import PageHeader from '@/components/ui/PageHeader.vue'
import SystemStats from '@/components/SystemStats.vue'

import { displayTitle } from '@/composables/displayTitle'
import { authFetch } from '@/composables/authFetch'
import { shortDate, todayInChannel, useCommercialApi } from '@/composables/commercialApi'
import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'
import { useLive } from '@/stores/live'

interface Attention {
    level: 'bad' | 'warn' | 'info'
    text: string
    to?: { name: string; query?: Record<string, string> }
    action?: string
}

const SOON_DAYS = 3
const REFRESH_MS = 30_000

const { t } = useI18n()
const api = useCommercialApi()
const authStore = useAuth()
const configStore = useConfig()
const live = useLive()

useHead({ title: computed(() => t('nav.dashboard')) })

const canManage = computed(() => authStore.role !== 'user')
const timezone = computed(() => configStore.timezone || 'UTC')
const coverage = ref<CoverageDay[]>([])
const spots = ref<CommercialSpot[]>([])
const campaigns = ref<CommercialCampaign[]>([])
const airings = ref<Airing[]>([])
const schedule = ref<CommercialWeekItem[]>([])
const windows = ref<CommercialBreakWindow[]>([])
const auto = ref<AutoPlanStatus | null>(null)
const program = ref<PlaylistItem[]>([])
const loaded = ref(false)
const showTech = ref(false)
let timer: ReturnType<typeof setInterval> | undefined

const channelId = computed(() => configStore.channels[configStore.i]?.id)
const today = computed(() => todayInChannel(timezone.value))

const next = computed(() => {
    const item = program.value[live.index + 1]

    if (!live.running || !item) {
        return null
    }

    const seconds = item.begin ?? 0
    const hours = String(Math.floor(seconds / 3600) % 24).padStart(2, '0')
    const minutes = String(Math.floor((seconds % 3600) / 60)).padStart(2, '0')

    return {
        title: displayTitle(item.title) || item.source.split('/').pop() || item.source,
        time: `${hours}:${minutes}`,
    }
})

const spotRows = computed(() => airings.value.filter((row) => row.kind === 'spot'))
const todayTotals = computed(() => ({
    planned: spotRows.value.length,
    aired: spotRows.value.filter((row) => row.result === 'played' || row.result === 'partial').length,
    missed: spotRows.value.filter((row) =>
        ['skipped', 'interrupted', 'fallback', 'failed', 'not_aired', 'live_missed'].includes(row.result),
    ).length,
}))

function daysFromToday(date: string | null) {
    return date
        ? Math.round(
              (new Date(`${date}T12:00:00`).getTime() - new Date(`${today.value}T12:00:00`).getTime()) / 86_400_000,
          )
        : 0
}

const attention = computed<Attention[]>(() => {
    const list: Attention[] = []

    for (const day of coverage.value.slice(0, 2)) {
        if (!day.playlist_exists) {
            list.push({
                level: 'bad',
                text: t(day.has_schedule ? 'dash.attention.dayNotBuilt' : 'dash.attention.dayNoSchedule', {
                    date: shortDate(day.date),
                }),
                to: { name: day.has_schedule ? 'commercial-today' : 'commercial-grid', query: { date: day.date } },
                action: t(day.has_schedule ? 'dash.attention.build' : 'dash.attention.setSchedule'),
            })
        }
    }

    for (const day of auto.value?.days ?? []) {
        if (day.state === 'failed' && !list.some((item) => item.text.includes(shortDate(day.plan_date)))) {
            list.push({
                level: 'warn',
                text: t('dash.attention.autoFailed', { date: shortDate(day.plan_date) }),
                to: { name: 'commercial-today', query: { date: day.plan_date } },
                action: t('dash.attention.see'),
            })
        }
    }

    const failedSpots = spots.value.filter((spot) => spot.status === 'failed').length

    if (failedSpots > 0) {
        list.push({
            level: 'warn',
            text: t('dash.attention.spotsFailed', { count: failedSpots }),
            to: { name: 'commercial-spots' },
            action: t('dash.attention.see'),
        })
    }

    for (const campaign of campaigns.value) {
        const ready = spots.value.filter((spot) => spot.campaign_id === campaign.id && spot.status === 'ready').length
        const ended = campaign.kind === 'campaign' && campaign.end_date !== null && campaign.end_date < today.value

        if (!ended && ready === 0) {
            list.push({
                level: 'warn',
                text: t('dash.attention.campaignNoSpots', { name: campaign.name }),
                to: { name: 'commercial-spots' },
                action: t('dash.attention.sendSpot'),
            })
        } else if (
            !ended &&
            campaign.kind === 'campaign' &&
            campaign.end_date !== null &&
            daysFromToday(campaign.end_date) <= SOON_DAYS
        ) {
            list.push({
                level: 'info',
                text: t('dash.attention.campaignEnding', {
                    name: campaign.name,
                    date: shortDate(campaign.end_date),
                }),
                to: { name: 'commercial-reports' },
                action: t('dash.attention.report'),
            })
        }
    }

    if (todayTotals.value.missed > 0) {
        list.push({
            level: 'warn',
            text: t('dash.attention.missedToday', { count: todayTotals.value.missed }),
            to: { name: 'commercial-today' },
            action: t('dash.attention.see'),
        })
    }

    return list
})

interface Step {
    done: boolean
    title: string
    text: string
    to: string
}

const steps = computed<Step[]>(() => [
    {
        done: schedule.value.length > 0,
        title: t('dash.steps.schedule.title'),
        text: t('dash.steps.schedule.text'),
        to: 'commercial-grid',
    },
    {
        done: windows.value.length > 0,
        title: t('dash.steps.breaks.title'),
        text: t('dash.steps.breaks.text'),
        to: 'commercial-breaks',
    },
    {
        done: campaigns.value.length > 0,
        title: t('dash.steps.campaign.title'),
        text: t('dash.steps.campaign.text'),
        to: 'commercial-campaigns',
    },
    {
        done: spots.value.some((spot) => spot.status === 'ready'),
        title: t('dash.steps.spot.title'),
        text: t('dash.steps.spot.text'),
        to: 'commercial-spots',
    },
    {
        done: coverage.value.some((day) => day.playlist_exists),
        title: t('dash.steps.build.title'),
        text: t('dash.steps.build.text'),
        to: 'commercial-today',
    },
])

const allDone = computed(() => steps.value.every((step) => step.done))

async function loadProgram(date: string) {
    try {
        const data = await authFetch<{ program?: PlaylistItem[] }>(`/api/playlist/${channelId.value}?date=${date}`)
        const start = configStore.playout.playlist?.startInSec ?? 0
        let begin = start

        program.value = (data.program ?? []).map((item) => {
            const result = { ...item, begin }

            begin += item.out - item.in

            return result
        })
    } catch {
        program.value = []
    }
}

async function load() {
    if (!channelId.value) {
        return
    }

    const attempt = async <T,>(call: Promise<T>, fallback: T) => {
        try {
            return await call
        } catch {
            return fallback
        }
    }

    const [days, spotList, campaignList, rows, template, breakList, autoStatus] = await Promise.all([
        attempt(api.get<CoverageDay[]>('/coverage?days=14'), [] as CoverageDay[]),
        attempt(api.spots(), [] as CommercialSpot[]),
        attempt(api.campaigns(), [] as CommercialCampaign[]),
        attempt(api.airlog(today.value, today.value), [] as Airing[]),
        attempt(api.get<CommercialWeekItem[]>('/week-template'), [] as CommercialWeekItem[]),
        attempt(api.breakWindows(), [] as CommercialBreakWindow[]),
        attempt(api.autoPlan(), null as AutoPlanStatus | null),
    ])

    coverage.value = days
    spots.value = spotList
    campaigns.value = campaignList
    airings.value = rows
    schedule.value = template
    windows.value = breakList
    auto.value = autoStatus
    loaded.value = true

    if (days[0]) {
        await loadProgram(days[0].date)
    }
}

onMounted(() => {
    void load()
    timer = setInterval(() => void load(), REFRESH_MS)
})

onBeforeUnmount(() => clearInterval(timer))
watch(channelId, () => void load())
</script>
<template>
    <div class="w-full px-3 sm:px-6 py-5 max-w-6xl mx-auto space-y-5">
        <PageHeader :title="t('dash.title')" :subtitle="t('dash.subtitle')" />

        <NowOnAir :next="next" />

        <section class="card bg-base-100 shadow p-5">
            <h2 class="text-xl font-bold">{{ t('dash.attention.title') }}</h2>
            <ul v-if="attention.length" class="mt-3 space-y-2">
                <li
                    v-for="(item, index) in attention"
                    :key="index"
                    class="flex flex-wrap items-center gap-3 rounded-lg border-l-4 bg-base-200 p-3"
                    :class="{
                        'border-error': item.level === 'bad',
                        'border-warning': item.level === 'warn',
                        'border-info': item.level === 'info',
                    }"
                >
                    <i
                        class="bi text-xl"
                        :class="{
                            'bi-exclamation-octagon-fill text-error': item.level === 'bad',
                            'bi-exclamation-triangle-fill text-warning': item.level === 'warn',
                            'bi-info-circle-fill text-info': item.level === 'info',
                        }"
                    />
                    <span class="grow min-w-48">{{ item.text }}</span>
                    <RouterLink v-if="item.to" :to="item.to" class="btn btn-sm">{{ item.action }}</RouterLink>
                </li>
            </ul>
            <p v-else-if="loaded" class="mt-3 flex items-center gap-2 text-success font-semibold">
                <i class="bi bi-check-circle-fill text-2xl" /> {{ t('dash.attention.allGood') }}
            </p>
        </section>

        <div class="grid gap-5 lg:grid-cols-3">
            <section class="card bg-base-100 shadow p-5 lg:col-span-2">
                <div class="flex items-center gap-2">
                    <h2 class="text-xl font-bold grow">{{ t('dash.coverage.title') }}</h2>
                    <RouterLink v-if="canManage" :to="{ name: 'commercial-grid' }" class="btn btn-sm btn-ghost">
                        {{ t('dash.coverage.edit') }}
                    </RouterLink>
                </div>
                <p class="text-sm opacity-70 mb-3">{{ t('dash.coverage.help') }}</p>
                <CoverageStrip v-if="coverage.length" :days="coverage" />
                <p v-else-if="loaded" class="opacity-70">{{ t('dash.coverage.none') }}</p>
            </section>

            <section class="card bg-base-100 shadow p-5">
                <h2 class="text-xl font-bold">{{ t('dash.today.title') }}</h2>
                <dl class="mt-3 grid grid-cols-3 gap-2 text-center">
                    <div class="rounded-lg bg-base-200 p-2">
                        <dd class="text-3xl font-bold">{{ todayTotals.planned }}</dd>
                        <dt class="text-xs opacity-70">{{ t('dash.today.planned') }}</dt>
                    </div>
                    <div class="rounded-lg bg-success/15 p-2">
                        <dd class="text-3xl font-bold text-success">{{ todayTotals.aired }}</dd>
                        <dt class="text-xs opacity-70">{{ t('dash.today.aired') }}</dt>
                    </div>
                    <div class="rounded-lg p-2" :class="todayTotals.missed ? 'bg-error/15' : 'bg-base-200'">
                        <dd class="text-3xl font-bold" :class="todayTotals.missed > 0 && 'text-error'">
                            {{ todayTotals.missed }}
                        </dd>
                        <dt class="text-xs opacity-70">{{ t('dash.today.missed') }}</dt>
                    </div>
                </dl>
                <RouterLink :to="{ name: 'commercial-today' }" class="btn btn-sm mt-4">
                    {{ t('dash.today.details') }}
                </RouterLink>
            </section>
        </div>

        <section v-if="canManage && loaded && !allDone" class="card bg-base-100 shadow p-5">
            <h2 class="text-xl font-bold">{{ t('dash.steps.title') }}</h2>
            <p class="text-sm opacity-70">{{ t('dash.steps.help') }}</p>
            <ol class="mt-3 space-y-2">
                <li v-for="(step, index) in steps" :key="step.to" class="flex items-start gap-3">
                    <span
                        class="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-sm font-bold"
                        :class="step.done ? 'bg-success text-success-content' : 'bg-base-300'"
                    >
                        <i v-if="step.done" class="bi bi-check-lg" />
                        <template v-else>{{ index + 1 }}</template>
                    </span>
                    <div class="grow min-w-0">
                        <div class="font-semibold" :class="step.done && 'line-through opacity-60'">
                            {{ step.title }}
                        </div>
                        <div class="text-sm opacity-70">{{ step.text }}</div>
                    </div>
                    <RouterLink v-if="!step.done" :to="{ name: step.to }" class="btn btn-sm btn-primary shrink-0">
                        {{ t('dash.steps.go') }}
                    </RouterLink>
                </li>
            </ol>
        </section>

        <section v-if="canManage" class="card bg-base-100 shadow">
            <button class="flex w-full items-center gap-2 p-5 text-left" @click="showTech = !showTech">
                <i class="bi" :class="showTech ? 'bi-chevron-down' : 'bi-chevron-right'" />
                <span class="font-semibold grow">{{ t('dash.tech.title') }}</span>
                <span class="text-sm opacity-60">{{ t('dash.tech.help') }}</span>
            </button>
            <div v-if="showTech" class="px-5 pb-5">
                <SystemStats v-if="configStore.channels.length > 0" />
            </div>
        </section>
    </div>
</template>
