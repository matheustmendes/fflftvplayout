<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'

import { shortDate } from '@/composables/commercialApi'

export interface CoverageDay {
    date: string
    weekday: number
    playlist_exists: boolean
    has_schedule: boolean
    auto_state: AutoPlanState | null
    on_air: boolean
}

defineProps<{ days: CoverageDay[] }>()

const { t, locale } = useI18n()
const router = useRouter()

function weekdayShort(date: string) {
    return new Intl.DateTimeFormat(locale.value, { weekday: 'short' })
        .format(new Date(`${date}T12:00:00`))
        .replace('.', '')
}

function dayNumber(date: string) {
    return date.slice(8, 10)
}

function kind(day: CoverageDay) {
    if (day.playlist_exists) {
        return 'ready'
    }

    return day.has_schedule ? 'soon' : 'missing'
}

const styles = {
    ready: 'bg-success/15 border-success text-base-content',
    soon: 'bg-warning/20 border-warning text-base-content',
    missing: 'bg-error/15 border-error text-base-content',
}

const icons = {
    ready: 'bi-check-circle-fill text-success',
    soon: 'bi-hourglass-split text-warning',
    missing: 'bi-exclamation-octagon-fill text-error',
}

function open(day: CoverageDay) {
    void router.push({ name: 'commercial-today', query: { date: day.date } })
}
</script>
<template>
    <div>
        <div class="grid grid-cols-7 sm:grid-cols-14 gap-1.5">
            <button
                v-for="day in days"
                :key="day.date"
                type="button"
                class="rounded-lg border-2 p-1.5 flex flex-col items-center gap-0.5 transition-transform hover:scale-105"
                :class="[styles[kind(day)], day.on_air && 'ring-2 ring-offset-2 ring-offset-base-100 ring-primary']"
                :title="`${shortDate(day.date)}: ${t(`dash.coverage.${kind(day)}`)}`"
                @click="open(day)"
            >
                <span class="text-xs uppercase opacity-70">{{ weekdayShort(day.date) }}</span>
                <span class="text-lg font-bold leading-none">{{ dayNumber(day.date) }}</span>
                <i class="bi text-base" :class="icons[kind(day)]" />
            </button>
        </div>
        <ul class="mt-3 flex flex-wrap gap-x-5 gap-y-1 text-sm opacity-80">
            <li><i class="bi bi-check-circle-fill text-success" /> {{ t('dash.coverage.ready') }}</li>
            <li><i class="bi bi-hourglass-split text-warning" /> {{ t('dash.coverage.soon') }}</li>
            <li><i class="bi bi-exclamation-octagon-fill text-error" /> {{ t('dash.coverage.missing') }}</li>
        </ul>
    </div>
</template>
