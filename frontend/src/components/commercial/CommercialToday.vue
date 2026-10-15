<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute } from 'vue-router'

import GenericModal from '@/components/utils/GenericModal.vue'

import { displayTitle } from '@/composables/displayTitle'
import { friendlyError } from '@/composables/friendlyError'
import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'
import {
    addDays,
    clockIn,
    formatDuration,
    shortDate,
    todayInChannel,
    useCommercialApi,
} from '@/composables/commercialApi'

const { t, te } = useI18n()
const api = useCommercialApi()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const canManage = computed(() => authStore.role !== 'user')
const timezone = computed(() => configStore.timezone || 'UTC')
const route = useRoute()
const requestedDate =
    typeof route.query.date === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(route.query.date) ? route.query.date : ''
const date = ref(requestedDate || todayInChannel(timezone.value))
const preview = ref<PlanPreview | null>(null)
const airings = ref<Airing[]>([])
const autoPlan = ref<AutoPlanStatus | null>(null)
const loading = ref(false)
const showApply = ref(false)
let refreshTimer: ReturnType<typeof setInterval> | undefined

const isToday = computed(() => date.value === todayInChannel(timezone.value))
const spotRows = computed(() => airings.value.filter((row) => row.kind === 'spot'))
const liveRows = computed(() => airings.value.filter((row) => row.kind === 'live'))
const hasError = computed(() => preview.value?.plan.warnings.some((warning) => warning.severity === 'error') ?? false)
const nextSpot = computed(() => {
    const now = new Date().toISOString()

    return spotRows.value.find((row) => row.result === 'pending' && (row.planned_start ?? '') >= now)
})
const timeline = computed(() => preview.value?.plan.items.filter((item) => item.kind !== 'day_filler') ?? [])
const dayFill = computed(() => {
    const plan = preview.value?.plan

    return plan ? Math.min(100, Math.round((plan.total_sec / plan.day_length_sec) * 100)) : 0
})

function warningText(warning: PlanWarning) {
    const key = `commercial.warning.${warning.code}`

    const window = preview.value?.plan.breaks.find((item) => item.window_id === warning.window_id)

    return te(key) ? t(key, { detail: warning.message, time: window?.nominal_time ?? '' }) : warning.message
}

function resultClass(result: AirResult) {
    switch (result) {
        case 'played':
            return 'badge-success text-white'
        case 'airing':
            return 'badge-info text-white'
        case 'pending':
            return 'badge-ghost'
        case 'partial':
            return 'badge-warning'
        default:
            return 'badge-error text-white'
    }
}

/** The day on the air is never changed by the planner, which is expected, not a problem. */
function autoLabel(day: AutoPlanDay) {
    return day.plan_date === autoPlan.value?.active_date && day.state === 'existing'
        ? t('commercial.auto.onAir')
        : t(`commercial.auto.state.${day.state}`)
}

function autoClass(day: AutoPlanDay) {
    return day.plan_date === autoPlan.value?.active_date && day.state === 'existing'
        ? 'badge-info text-white'
        : autoStateClass(day.state)
}

function autoStateClass(state: AutoPlanState) {
    switch (state) {
        case 'created':
        case 'updated':
        case 'unchanged':
            return 'badge-success text-white'
        case 'failed':
            return 'badge-error text-white'
        default:
            return 'badge-warning'
    }
}

function kindLabel(kind: PlanItemKind) {
    return t(`commercial.today.kind.${kind}`)
}

async function load() {
    loading.value = true

    try {
        const [plan, rows, auto] = await Promise.all([
            api.plan(date.value),
            api.airlog(date.value, date.value),
            api.autoPlan(),
        ])

        preview.value = plan
        airings.value = rows
        autoPlan.value = auto
    } catch (error) {
        indexStore.msgAlert('error', friendlyError(error), 6)
    } finally {
        loading.value = false
    }
}

async function apply(confirmed: boolean) {
    showApply.value = false

    if (!confirmed) {
        return
    }

    try {
        const result = await api.applyPlan(date.value, true)

        indexStore.msgAlert('success', result.message, 4)
        await load()
    } catch (error) {
        indexStore.msgAlert('error', friendlyError(error), 6)
    }
}

function startApply() {
    if (preview.value?.playlist_exists) {
        showApply.value = true
    } else {
        void apply(true)
    }
}

onMounted(() => {
    void load()
    refreshTimer = setInterval(() => {
        if (isToday.value) {
            void load()
        }
    }, 15000)
})

onBeforeUnmount(() => clearInterval(refreshTimer))
watch(date, load)
watch(() => configStore.i, load)
</script>
<template>
    <div class="space-y-5">
        <div class="flex flex-wrap items-center gap-2">
            <button class="btn btn-sm" :title="t('commercial.today.previous')" @click="date = addDays(date, -1)">
                <i class="bi bi-chevron-left" />
            </button>
            <input v-model="date" type="date" class="input input-sm" />
            <button class="btn btn-sm" :title="t('commercial.today.next')" @click="date = addDays(date, 1)">
                <i class="bi bi-chevron-right" />
            </button>
            <button v-if="!isToday" class="btn btn-sm" @click="date = todayInChannel(timezone)">
                {{ t('commercial.today.goToday') }}
            </button>
            <button class="btn btn-sm" :disabled="loading" @click="load">
                <i class="bi bi-arrow-clockwise" /> {{ t('commercial.refresh') }}
            </button>
            <div class="grow" />
            <button
                v-if="canManage && preview"
                class="btn btn-sm btn-accent"
                :disabled="hasError || loading"
                @click="startApply"
            >
                {{ preview.playlist_exists ? t('commercial.today.applyAgain') : t('commercial.today.apply') }}
            </button>
        </div>

        <div v-if="preview" class="space-y-2">
            <div v-if="!preview.playlist_exists" class="alert alert-warning">
                <i class="bi bi-exclamation-triangle" /> {{ t('commercial.today.noPlaylist') }}
            </div>
            <div
                v-for="(warning, index) in preview.plan.warnings"
                :key="index"
                class="alert"
                :class="warning.severity === 'error' ? 'alert-error' : 'alert-warning'"
            >
                <i class="bi" :class="warning.severity === 'error' ? 'bi-x-octagon' : 'bi-exclamation-triangle'" />
                {{ warningText(warning) }}
            </div>
        </div>

        <div v-if="nextSpot" class="alert bg-base-100 border border-accent">
            <i class="bi bi-clock" />
            {{
                t('commercial.today.next_spot', {
                    time: clockIn(nextSpot.planned_start, timezone),
                    title: nextSpot.spot_title,
                    campaign: nextSpot.campaign_name,
                })
            }}
        </div>

        <section
            v-if="autoPlan && autoPlan.settings.enabled && autoPlan.days.length"
            class="card bg-base-100 shadow p-4"
        >
            <h2 class="font-bold text-lg mb-1">{{ t('commercial.auto.upcoming') }}</h2>
            <p class="text-xs opacity-60 mb-2">{{ t('commercial.auto.upcomingHelp') }}</p>
            <ul class="text-sm divide-y divide-base-300">
                <li v-for="day in autoPlan.days" :key="day.plan_date" class="flex flex-wrap items-center gap-2 py-1">
                    <span class="w-24 tabular-nums">{{ shortDate(day.plan_date) }}</span>
                    <span class="badge badge-sm" :class="autoClass(day)">{{ autoLabel(day) }}</span>
                    <span v-if="day.state === 'failed'" class="text-error text-xs grow min-w-0">{{ day.message }}</span>
                </li>
            </ul>
        </section>

        <section v-if="preview" class="card bg-base-100 shadow p-4">
            <h2 class="font-bold text-lg mb-2">{{ t('commercial.today.breaks') }}</h2>
            <p v-if="!preview.plan.breaks.length" class="opacity-70">{{ t('commercial.today.noBreaks') }}</p>
            <div v-else class="overflow-x-auto">
                <table class="table table-sm">
                    <thead>
                        <tr>
                            <th>{{ t('commercial.today.asked') }}</th>
                            <th>{{ t('commercial.today.startsAt') }}</th>
                            <th>{{ t('commercial.today.length') }}</th>
                            <th>{{ t('commercial.today.spots') }}</th>
                            <th>{{ t('commercial.today.gap') }}</th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr v-for="item in preview.plan.breaks" :key="item.window_id">
                            <td>{{ item.nominal_time }}</td>
                            <td>{{ item.start_time }}</td>
                            <td>{{ formatDuration(item.duration_sec) }}</td>
                            <td>{{ item.spots }} ({{ formatDuration(item.spots_sec) }})</td>
                            <td :class="item.gap_sec > 5 && 'text-warning font-semibold'">
                                {{ formatDuration(item.gap_sec) }}
                            </td>
                        </tr>
                    </tbody>
                </table>
            </div>
            <div class="mt-3 text-sm opacity-80">
                {{ t('commercial.today.dayFill', { percent: dayFill }) }}
                <progress class="progress progress-accent w-full" :value="dayFill" max="100" />
            </div>
        </section>

        <section class="card bg-base-100 shadow p-4">
            <h2 class="font-bold text-lg mb-2">
                {{ t('commercial.today.aired') }} <span class="opacity-60 text-sm">{{ shortDate(date) }}</span>
            </h2>
            <p v-if="!spotRows.length" class="opacity-70">{{ t('commercial.today.noAirings') }}</p>
            <div v-else class="overflow-x-auto">
                <table class="table table-sm">
                    <thead>
                        <tr>
                            <th>{{ t('commercial.today.planned') }}</th>
                            <th>{{ t('commercial.today.real') }}</th>
                            <th>{{ t('commercial.spots.title') }}</th>
                            <th>{{ t('commercial.campaigns.campaign') }}</th>
                            <th>{{ t('commercial.today.result') }}</th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr v-for="row in spotRows" :key="row.id">
                            <td>{{ clockIn(row.planned_start, timezone) }}</td>
                            <td>{{ clockIn(row.actual_start, timezone) }}</td>
                            <td>{{ row.spot_title }}</td>
                            <td>{{ row.advertiser_name }} / {{ row.campaign_name }}</td>
                            <td>
                                <span class="badge badge-sm" :class="resultClass(row.result)">
                                    {{ t(`commercial.result.${row.result}`) }}
                                </span>
                            </td>
                        </tr>
                    </tbody>
                </table>
            </div>
            <div v-if="liveRows.length" class="mt-3 text-sm">
                <div class="font-semibold">{{ t('commercial.today.live') }}</div>
                <div v-for="row in liveRows" :key="row.id" class="opacity-80">
                    {{ clockIn(row.actual_start, timezone) }}
                    <template v-if="row.actual_end"> — {{ clockIn(row.actual_end, timezone) }}</template>
                    <template v-else> — {{ t('commercial.today.liveOn') }}</template>
                </div>
            </div>
        </section>

        <section v-if="preview" class="card bg-base-100 shadow p-4">
            <h2 class="font-bold text-lg mb-2">{{ t('commercial.today.timeline') }}</h2>
            <ul class="text-sm divide-y divide-base-300">
                <li v-for="(item, index) in timeline" :key="index" class="flex gap-3 py-1">
                    <span class="w-20 shrink-0 tabular-nums">{{ item.start_time }}</span>
                    <span
                        class="badge badge-sm shrink-0 w-24"
                        :class="item.kind === 'spot' ? 'badge-accent' : item.kind === 'program' ? 'badge-ghost' : ''"
                    >
                        {{ kindLabel(item.kind) }}
                    </span>
                    <span class="truncate grow">{{ displayTitle(item.title) || item.source }}</span>
                    <span class="w-14 text-right opacity-70">{{ formatDuration(item.duration) }}</span>
                </li>
            </ul>
        </section>

        <GenericModal
            :show="showApply"
            :title="t('commercial.today.replaceTitle')"
            :text="t('commercial.today.replaceText', { date: shortDate(date) })"
            :modal-action="apply"
        />
    </div>
</template>
