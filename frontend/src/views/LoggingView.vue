<script setup lang="ts">
import dayjs from 'dayjs'
import customParseFormat from 'dayjs/plugin/customParseFormat.js'
import LocalizedFormat from 'dayjs/plugin/localizedFormat.js'
import timezone from 'dayjs/plugin/timezone.js'
import utc from 'dayjs/plugin/utc.js'
import { computed, nextTick, ref, onMounted, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { useI18n } from 'vue-i18n'
import { useHead } from '@unhead/vue'
import { VueDatePicker } from '@vuepic/vue-datepicker'
import { de, enUS, ptBR, ru } from 'date-fns/locale'
import '@vuepic/vue-datepicker/dist/main.css'

import 'dayjs/locale/de'
import 'dayjs/locale/en'
import 'dayjs/locale/es'
import 'dayjs/locale/pt-br'
import 'dayjs/locale/ru'

dayjs.extend(customParseFormat)
dayjs.extend(LocalizedFormat)
dayjs.extend(timezone)
dayjs.extend(utc)

import EmptyState from '@/components/ui/EmptyState.vue'
import PageHeader from '@/components/ui/PageHeader.vue'

import { authFetch, authFetchRaw } from '@/composables/authFetch'
import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'

type LogLevel = 'DEBUG' | 'INFO' | 'WARN' | 'ERROR' | 'FATAL'
type FilterChoice = 'all' | 'warnings' | 'problems'

interface LogEntry {
    id: number
    time: string
    level: LogLevel
    // Message part of the line. The server already sends it as HTML with colored spans.
    html: string
    plain: string
}

const PAGE_SIZE = 500
const LEVEL_PATTERN = /\[ ?(DEBUG|INFO|WARN|ERROR|FATAL)\]/
const FILTER_LEVEL: Record<FilterChoice, LogLevel> = { all: 'DEBUG', warnings: 'WARN', problems: 'ERROR' }

const { locale, t } = useI18n()
const indexStore = useIndex()

useHead({
    title: computed(() => t('nav.history')),
})

const { i } = storeToRefs(useConfig())
const authStore = useAuth()
const configStore = useConfig()
const currentLog = ref('')
const loading = ref(false)
const listDate = ref(dayjs().tz(configStore.timezone).format('YYYY-MM-DD'))
const lang = ref()
const searchText = ref('')
const visibleLimit = ref(PAGE_SIZE)
const newestFirst = ref(readStored('log_newest_first') !== 'false')

const errorLevel = ref(readStored('error_level') || 'INFO')

switch (locale.value) {
    case 'de':
        lang.value = de
        break
    case 'pt-br':
        lang.value = ptBR
        break
    case 'ru':
        lang.value = ru
        break
    default:
        lang.value = enUS
}

const dateFormat = computed(() => (locale.value === 'pt-br' ? 'EEE, dd/MM/yyyy' : 'EEE dd. LLL yyyy'))
const today = computed(() => dayjs().tz(configStore.timezone).format('YYYY-MM-DD'))
const isToday = computed(() => listDate.value === today.value)

const filterChoice = computed<FilterChoice>({
    get() {
        const level = indexStore.severityLevels[errorLevel.value] ?? 0

        if (level >= indexStore.severityLevels.ERROR!) {
            return 'problems'
        }

        return level >= indexStore.severityLevels.WARN! ? 'warnings' : 'all'
    },
    set(choice) {
        errorLevel.value = FILTER_LEVEL[choice]
    },
})

const entries = computed(() => parseLog(currentLog.value))
const counts = computed(() => {
    const result = { all: entries.value.length, warnings: 0, problems: 0 }

    for (const entry of entries.value) {
        const level = severity(entry.level)

        if (level >= severity('ERROR')) {
            result.problems++
        }

        if (level >= severity('WARN')) {
            result.warnings++
        }
    }

    return result
})

const filteredEntries = computed(() => {
    const minLevel = severity(FILTER_LEVEL[filterChoice.value])
    const needle = searchText.value.trim().toLowerCase()
    const list = entries.value.filter(
        (entry) => severity(entry.level) >= minLevel && (!needle || entry.plain.toLowerCase().includes(needle)),
    )

    return newestFirst.value ? [...list].reverse() : list
})
const visibleEntries = computed(() => filteredEntries.value.slice(0, visibleLimit.value))

onMounted(async () => {
    await getLog()
})

watch([listDate, i], () => {
    getLog()
})

watch(errorLevel, (newValue) => {
    storeValue('error_level', newValue)
})

watch(newestFirst, (newValue) => {
    storeValue('log_newest_first', String(newValue))
    nextTick(scrollTo)
})

watch([errorLevel, searchText], () => {
    visibleLimit.value = PAGE_SIZE
})

function readStored(key: string): string | null {
    try {
        return localStorage.getItem(key)
    } catch {
        return null
    }
}

function storeValue(key: string, value: string) {
    try {
        localStorage.setItem(key, value)
    } catch {
        // Remembering the choice is a convenience only.
    }
}

function severity(level: string): number {
    return indexStore.severityLevels[level] ?? 0
}

function stripTags(html: string): string {
    return html
        .replace(/<[^>]+>/g, '')
        .replace(/&lt;/g, '<')
        .replace(/&gt;/g, '>')
        .replace(/&quot;/g, '"')
        .replace(/&amp;/g, '&')
}

function parseLog(raw: string): LogEntry[] {
    const result: LogEntry[] = []
    const lines = raw.trim() ? raw.trim().split(/\r?\n/) : []
    let lastLevel: LogLevel = 'INFO'

    lines.forEach((line, index) => {
        const plain = stripTags(line)
        const levelMatch = plain.match(LEVEL_PATTERN)
        const timeMatch = plain.match(/^\[(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2})/)

        if (levelMatch) {
            lastLevel = levelMatch[1] as LogLevel
        }

        // Drop the timestamp and level markers from the HTML; they get their own columns.
        const message = levelMatch
            ? line.replace(/^.*?\[ ?(?:DEBUG|INFO|WARN|ERROR|FATAL)\](?:<\/span>)?\s?/, '')
            : line

        result.push({
            id: index,
            time: levelMatch && timeMatch ? (timeMatch[1] ?? '') : '',
            level: lastLevel,
            html: message,
            plain,
        })
    })

    return result
}

function levelLabel(level: LogLevel): string {
    switch (level) {
        case 'FATAL':
            return t('history2.level.fatal')
        case 'ERROR':
            return t('history2.level.error')
        case 'WARN':
            return t('history2.level.warn')
        case 'DEBUG':
            return t('history2.level.debug')
        default:
            return t('history2.level.info')
    }
}

function levelIcon(level: LogLevel): string {
    switch (level) {
        case 'FATAL':
        case 'ERROR':
            return 'bi-x-octagon-fill'
        case 'WARN':
            return 'bi-exclamation-triangle-fill'
        case 'DEBUG':
            return 'bi-bug-fill'
        default:
            return 'bi-info-circle-fill'
    }
}

function levelClasses(level: LogLevel): { row: string; badge: string } {
    switch (level) {
        case 'FATAL':
        case 'ERROR':
            return { row: 'border-error bg-error/10', badge: 'bg-error text-error-content' }
        case 'WARN':
            return { row: 'border-warning bg-warning/10', badge: 'bg-warning text-warning-content' }
        case 'DEBUG':
            return { row: 'border-base-300', badge: 'bg-base-300 text-base-content' }
        default:
            return { row: 'border-info/40', badge: 'bg-info/15 text-info' }
    }
}

function scrollTo() {
    const parent = document.getElementById('log-container')

    if (parent) {
        parent.scrollTop = newestFirst.value ? 0 : parent.scrollHeight
    }
}

function shiftDay(days: number) {
    listDate.value = dayjs(listDate.value).add(days, 'day').format('YYYY-MM-DD')
}

function logDateParam(): string {
    return isToday.value ? '' : listDate.value
}

async function getLog() {
    loading.value = true
    visibleLimit.value = PAGE_SIZE

    await authFetch<string>(
        `/api/log/${configStore.channels[configStore.i]?.id}?date=${logDateParam()}&timezone=${encodeURIComponent(
            configStore.timezone,
        )}`,
        {
            method: 'GET',
            headers: authStore.authHeader,
            responseType: 'text',
        },
    )
        .then((data) => {
            currentLog.value = data

            nextTick(() => {
                scrollTo()
            })
        })
        .catch(() => {
            currentLog.value = ''
        })

    loading.value = false
}

async function downloadLog() {
    const id = configStore.channels[configStore.i]?.id
    let date = logDateParam()

    const response = await authFetchRaw(
        `/api/log/${id}?date=${date}&timezone=${encodeURIComponent(configStore.timezone)}&download=true`,
        {
            method: 'GET',
            headers: authStore.authHeader,
        },
    )

    if (!response.ok) {
        indexStore.msgAlert('error', await response.text(), 7)
        return
    }

    if (date) {
        date = `_${date}`
    }

    const blob = await response.blob()
    const url = URL.createObjectURL(blob)
    const link = document.createElement('a')

    link.href = url
    link.download = `ffplayout_${id}${date}.log`
    document.body.appendChild(link)
    link.click()
    document.body.removeChild(link)
    URL.revokeObjectURL(url)
}
</script>
<template>
    <div class="w-full px-3 sm:px-6 py-5 max-w-6xl mx-auto">
        <PageHeader :title="t('nav.history')" :subtitle="t('history2.subtitle')">
            <template #actions>
                <button class="btn btn-primary" :disabled="loading" @click="getLog()">
                    <i class="bi bi-arrow-repeat" :class="loading && 'animate-spin'" />
                    {{ t('history2.refresh') }}
                </button>
                <button class="btn" @click="downloadLog">
                    <i class="bi bi-download" />
                    {{ t('history2.download') }}
                </button>
            </template>
        </PageHeader>

        <section class="card bg-base-100 shadow p-4 sm:p-5">
            <div class="flex flex-wrap items-end gap-x-6 gap-y-4">
                <!-- What to show -->
                <fieldset>
                    <legend class="text-sm font-bold mb-1">{{ t('history2.show') }}</legend>
                    <div class="flex flex-wrap gap-2">
                        <button
                            v-for="choice in ['all', 'warnings', 'problems'] as const"
                            :key="choice"
                            type="button"
                            class="btn"
                            :class="filterChoice === choice && 'btn-primary'"
                            :aria-pressed="filterChoice === choice"
                            @click="filterChoice = choice"
                        >
                            {{ t(`history2.filter.${choice}`) }}
                            <span
                                class="badge badge-sm"
                                :class="
                                    filterChoice === choice
                                        ? 'bg-white/25 text-primary-content border-0'
                                        : choice !== 'all' && counts[choice] > 0
                                          ? choice === 'problems'
                                              ? 'badge-error'
                                              : 'badge-warning'
                                          : 'badge-ghost'
                                "
                            >
                                {{ counts[choice] }}
                            </span>
                        </button>
                    </div>
                </fieldset>

                <!-- Which day -->
                <fieldset class="w-full min-w-0 sm:w-auto">
                    <legend class="text-sm font-bold mb-1">{{ t('history2.day') }}</legend>
                    <div class="flex w-full items-center gap-1">
                        <button
                            type="button"
                            class="btn btn-square"
                            :title="t('history2.prevDay')"
                            :aria-label="t('history2.prevDay')"
                            @click="shiftDay(-1)"
                        >
                            <i class="bi bi-chevron-left" />
                        </button>
                        <VueDatePicker
                            v-model="listDate"
                            :clearable="false"
                            :hide-navigation="['time']"
                            :action-row="{ showCancel: false, showSelect: false, showPreview: false }"
                            :formats="{ day: 'dd', input: dateFormat }"
                            :time-config="{ enableTimePicker: false }"
                            :max-date="today"
                            model-type="yyyy-MM-dd"
                            auto-apply
                            class="min-w-0 grow sm:grow-0 sm:w-64"
                            :locale="lang"
                            :dark="indexStore.darkMode"
                            :ui="{ input: 'input w-full! h-10! text-center' }"
                            required
                        />
                        <button
                            type="button"
                            class="btn btn-square"
                            :disabled="isToday"
                            :title="t('history2.nextDay')"
                            :aria-label="t('history2.nextDay')"
                            @click="shiftDay(1)"
                        >
                            <i class="bi bi-chevron-right" />
                        </button>
                        <button type="button" class="btn" :disabled="isToday" @click="listDate = today">
                            {{ t('history2.today') }}
                        </button>
                    </div>
                </fieldset>

                <!-- Search -->
                <label class="grow min-w-56">
                    <span class="block text-sm font-bold mb-1">{{ t('history2.searchLabel') }}</span>
                    <span class="input w-full">
                        <i class="bi bi-search opacity-60" />
                        <input v-model="searchText" type="search" :placeholder="t('history2.searchPlaceholder')" />
                    </span>
                </label>
            </div>

            <div class="mt-3 flex flex-wrap items-center gap-x-4 gap-y-1 text-sm opacity-80">
                <span>
                    {{ t('history2.showing', { shown: visibleEntries.length, total: filteredEntries.length }) }}
                </span>
                <label class="flex items-center gap-2 cursor-pointer">
                    <input v-model="newestFirst" type="checkbox" class="checkbox checkbox-sm" />
                    {{ t('history2.newestFirst') }}
                </label>
            </div>
        </section>

        <section class="relative card bg-base-100 shadow mt-5 overflow-hidden">
            <div v-if="loading" class="absolute inset-0 z-10 flex justify-center pt-16 bg-base-100/70">
                <span class="loading loading-spinner loading-lg text-primary" />
            </div>

            <EmptyState
                v-if="!loading && entries.length === 0"
                icon="bi-journal-text"
                :title="t('history2.emptyTitle')"
                :text="t('history2.emptyText')"
            >
                <button v-if="!isToday" class="btn btn-primary" @click="listDate = today">
                    {{ t('history2.goToday') }}
                </button>
            </EmptyState>

            <EmptyState
                v-else-if="!loading && filteredEntries.length === 0"
                :icon="filterChoice === 'all' || searchText ? 'bi-search' : 'bi-check-circle'"
                :title="
                    searchText
                        ? t('history2.noMatchTitle')
                        : filterChoice === 'problems'
                          ? t('history2.noProblemsTitle')
                          : t('history2.noWarningsTitle')
                "
                :text="searchText ? t('history2.noMatchText') : t('history2.noProblemsText')"
            >
                <button v-if="searchText" class="btn" @click="searchText = ''">{{ t('history2.clearSearch') }}</button>
                <button v-if="filterChoice !== 'all'" class="btn btn-primary" @click="filterChoice = 'all'">
                    {{ t('history2.showAll') }}
                </button>
            </EmptyState>

            <div
                v-if="visibleEntries.length > 0"
                id="log-container"
                class="max-h-[calc(100vh-22rem)] min-h-80 overflow-y-auto"
            >
                <ul id="log-content">
                    <li
                        v-for="entry in visibleEntries"
                        :key="entry.id"
                        class="flex flex-wrap items-start gap-x-3 gap-y-1 border-l-4 border-b border-b-base-200 px-3 py-2"
                        :class="levelClasses(entry.level).row"
                    >
                        <div class="flex items-center gap-2 sm:w-48 shrink-0">
                            <span
                                class="inline-flex w-28 items-center justify-center gap-1 rounded-full px-2 py-0.5 text-xs font-bold whitespace-nowrap"
                                :class="levelClasses(entry.level).badge"
                            >
                                <i class="bi" :class="levelIcon(entry.level)" />
                                {{ levelLabel(entry.level) }}
                            </span>
                            <span v-if="entry.time" class="text-sm tabular-nums opacity-70" :title="entry.time">
                                {{ entry.time.slice(11, 19) }}
                            </span>
                        </div>
                        <!-- eslint-disable-next-line vue/no-v-html -->
                        <div class="grow basis-64 min-w-0 font-mono text-[13px] break-words" v-html="entry.html" />
                    </li>
                </ul>
                <div v-if="filteredEntries.length > visibleEntries.length" class="p-4 text-center">
                    <button class="btn" @click="visibleLimit += PAGE_SIZE">
                        {{ t('history2.showMore') }}
                    </button>
                </div>
            </div>
        </section>
    </div>
</template>
<style>
.log-gray {
    color: #666864;
}

.log-number {
    color: var(--my-yellow);
}

.log-addr {
    color: var(--my-purple);
    font-weight: 500;
}

.log-cmd {
    color: var(--my-blue);
}

.level-info {
    color: var(--my-green);
}

.level-warning {
    color: var(--color-warning);
}

.level-error {
    color: var(--color-error);
}

.level-fatal {
    color: var(--color-error);
    font-weight: 700;
}

.level-debug {
    color: #6e99c7;
}

.log-decoder {
    color: #56efff;
}

:root[data-theme='ffp-light'] .log-decoder {
    color: #0e7490;
}

.log-encoder {
    color: #45ccee;
}

.log-server {
    color: #23cbdd;
}

.log-validate {
    color: #23cbdd;
}
</style>
