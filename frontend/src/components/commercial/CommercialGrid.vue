<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import GenericModal from '@/components/utils/GenericModal.vue'
import StorageFilePicker from '@/components/commercial/StorageFilePicker.vue'
import WeekdayTabs from '@/components/commercial/WeekdayTabs.vue'
import type { PickedFile } from '@/components/commercial/StorageFilePicker.vue'

import ActionSheet from '@/components/ui/ActionSheet.vue'
import { friendlyError } from '@/composables/friendlyError'
import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'
import { formatDuration, WEEKDAYS, useCommercialApi } from '@/composables/commercialApi'

interface GridItem {
    source: string
    title: string
    duration: number
    /** `HH:MM` the program must start at; empty follows the program before it. */
    fixed: string
}

const DAY_SECONDS = 86400

const { t } = useI18n()
const api = useCommercialApi()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const canManage = computed(() => authStore.role !== 'user')
const weekday = ref(new Date().getDay() || 7)
const items = ref<GridItem[]>([])
const saved = ref('[]')
const showPicker = ref(false)
const showCopy = ref(false)
const pendingDay = ref<number | null>(null)
const dragFrom = ref<number | null>(null)
const dragOver = ref<number | null>(null)
// Tela de toque: arrastar não funciona com o dedo, então há botões e uma folha de ações.
const touchScreen = window.matchMedia('(pointer: coarse)').matches
const sheetFor = ref<number | null>(null)

const dirty = computed(() => JSON.stringify(items.value) !== saved.value)
const totalSec = computed(() => timeline.value.end - dayStart.value)
const dayLength = computed(() => configStore.playlistLength || DAY_SECONDS)
const overflow = computed(() => totalSec.value - dayLength.value)
const dayStart = computed(() => configStore.playout.playlist?.startInSec ?? 0)

function fixedSeconds(time: string): number | null {
    const [hours, minutes] = time.split(':').map(Number)

    if (!time || Number.isNaN(hours) || Number.isNaN(minutes)) {
        return null
    }

    // A fixed time counts from the start of the day of the channel.
    return (
        dayStart.value + ((((hours * 3600 + minutes * 60 - dayStart.value) % DAY_SECONDS) + DAY_SECONDS) % DAY_SECONDS)
    )
}

/**
 * When each program starts on the clock, from the start of the day: a program
 * with a fixed time waits for it (the gap is filler) and, when what comes before
 * it runs over, it starts late and the row says so.
 */
const timeline = computed(() => {
    let at = dayStart.value

    const rows = items.value.map((item) => {
        const target = fixedSeconds(item.fixed)
        const gap = target !== null && target > at ? target - at : 0
        const late = target !== null && at - target > 1 ? at - target : 0
        const begin = at + gap

        at = begin + item.duration

        return { begin, gap, late }
    })

    return { rows, end: at }
})

const starts = computed(() => timeline.value.rows.map((row) => row.begin))

function clock(seconds: number) {
    const day = ((seconds % 86400) + 86400) % 86400
    const pad = (value: number) => String(Math.floor(value)).padStart(2, '0')

    return `${pad(day / 3600)}:${pad((day % 3600) / 60)}:${pad(day % 60)}`
}

const endClock = computed(() => clock(dayStart.value + totalSec.value))
const COLORS = ['bg-primary', 'bg-accent', 'bg-success', 'bg-info', 'bg-warning']

function fail(error: unknown) {
    indexStore.msgAlert('error', friendlyError(error), 6)
}

async function load() {
    try {
        const rows = await api.weekDay(weekday.value)

        items.value = rows.map((row) => ({
            source: row.source,
            title: row.title,
            duration: row.out_sec - row.in_sec,
            fixed: (row.fixed_start ?? '').slice(0, 5),
        }))
        saved.value = JSON.stringify(items.value)
    } catch (error) {
        fail(error)
    }
}

function changeDay(day: number) {
    if (dirty.value) {
        pendingDay.value = day
    } else {
        weekday.value = day
    }
}

function answerDiscard(confirmed: boolean) {
    const day = pendingDay.value

    pendingDay.value = null

    if (confirmed && day !== null) {
        weekday.value = day
    }
}

function move(index: number, by: number) {
    const target = index + by

    if (target < 0 || target >= items.value.length) {
        return
    }

    const copy = [...items.value]
    const [moved] = copy.splice(index, 1)

    copy.splice(target, 0, moved!)
    items.value = copy
}

function onDrop(target: number) {
    const from = dragFrom.value

    dragFrom.value = null
    dragOver.value = null

    if (from === null || from === target) {
        return
    }

    const copy = [...items.value]
    const [moved] = copy.splice(from, 1)

    copy.splice(target, 0, moved!)
    items.value = copy
}

function remove(index: number) {
    items.value = items.value.filter((_, position) => position !== index)
}

function addPicked(files: PickedFile[]) {
    showPicker.value = false
    items.value = [
        ...items.value,
        ...files.map((file) => ({
            source: file.path,
            title: file.name.replace(/\.[^.]+$/, ''),
            duration: file.duration,
            fixed: '',
        })),
    ]
}

const payload = () =>
    items.value.map((item) => ({ source: item.source, title: item.title, fixed_start: item.fixed || null }))

async function save() {
    try {
        await api.saveWeekDay(weekday.value, payload())
        indexStore.msgAlert('success', t('commercial.saved'), 3)
        await load()
    } catch (error) {
        fail(error)
    }
}

async function copyToOthers(confirmed: boolean) {
    showCopy.value = false

    if (!confirmed) {
        return
    }

    try {
        for (const day of WEEKDAYS.filter((value) => value !== weekday.value)) {
            await api.saveWeekDay(day, payload())
        }

        indexStore.msgAlert('success', t('commercial.grid.copied'), 4)
    } catch (error) {
        fail(error)
    }
}

onMounted(load)
watch(weekday, load)
watch(() => configStore.i, load)
</script>
<template>
    <div class="space-y-4">
        <p class="text-sm opacity-80">{{ t('commercial.grid.help') }}</p>
        <WeekdayTabs :model-value="weekday" @update:model-value="changeDay" />

        <section class="card bg-base-100 shadow p-4">
            <div class="flex flex-wrap items-center gap-2 mb-2">
                <h2 class="font-bold text-lg grow">{{ t(`commercial.weekday.${weekday}`) }}</h2>
                <span class="text-sm" :class="overflow > 0 ? 'text-error font-semibold' : 'opacity-70'">
                    {{ t('commercial.grid.total', { time: formatDuration(totalSec) }) }}
                    <template v-if="overflow > 0">
                        — {{ t('commercial.grid.over', { time: formatDuration(overflow) }) }}</template
                    >
                    <template v-else> — {{ t('commercial.grid.left', { time: formatDuration(-overflow) }) }}</template>
                </span>
            </div>

            <div v-if="overflow > 0" class="alert alert-error mb-3">
                <i class="bi bi-exclamation-octagon-fill" />
                {{ t('commercial.grid.problemOver', { time: formatDuration(overflow) }) }}
            </div>
            <div v-else-if="items.length && overflow < -dayLength * 0.5" class="alert alert-warning mb-3">
                <i class="bi bi-exclamation-triangle-fill" />
                {{ t('commercial.grid.problemShort', { time: formatDuration(-overflow) }) }}
            </div>
            <p v-if="!items.length" class="opacity-70">{{ t('commercial.grid.empty') }}</p>
            <div v-else>
                <div class="mb-3">
                    <div
                        class="flex h-5 w-full overflow-hidden rounded-full bg-base-300"
                        :title="t('commercial.grid.barHelp')"
                    >
                        <div
                            v-for="(item, index) in items"
                            :key="`bar-${index}`"
                            class="h-full border-r border-base-100"
                            :class="COLORS[index % COLORS.length]"
                            :style="{ width: `${Math.min(100, (item.duration * 100) / dayLength)}%` }"
                            :title="`${item.title} (${formatDuration(item.duration)})`"
                        />
                    </div>
                    <div class="mt-1 flex justify-between text-xs opacity-60">
                        <span>{{ clock(dayStart) }}</span>
                        <span>{{ t('commercial.grid.endsAt', { time: endClock }) }}</span>
                    </div>
                </div>
                <ol class="divide-y divide-base-300 text-sm">
                    <li
                        v-for="(item, index) in items"
                        :key="`${index}-${item.source}`"
                        class="flex flex-wrap items-center gap-x-2 gap-y-1 rounded py-2 transition-colors sm:flex-nowrap sm:py-1.5"
                        :class="[
                            dragOver === index && dragFrom !== index && 'bg-primary/10',
                            dragFrom === index && 'opacity-40',
                        ]"
                        :draggable="canManage && !touchScreen"
                        @dragstart="dragFrom = index"
                        @dragend=";((dragFrom = null), (dragOver = null))"
                        @dragover.prevent="dragOver = index"
                        @drop.prevent="onDrop(index)"
                    >
                        <i
                            v-if="canManage"
                            class="bi bi-grip-vertical cursor-grab text-lg opacity-50 hidden sm:inline"
                            :title="t('commercial.grid.dragHelp')"
                        />
                        <span class="w-6 text-right opacity-60">{{ index + 1 }}</span>
                        <span
                            class="w-20 shrink-0 tabular-nums"
                            :class="timeline.rows[index]?.late ? 'text-error font-semibold' : 'opacity-70'"
                            :title="
                                timeline.rows[index]?.late
                                    ? t('commercial.grid.late', { time: formatDuration(timeline.rows[index].late) })
                                    : timeline.rows[index]?.gap
                                      ? t('commercial.grid.gap', { time: formatDuration(timeline.rows[index].gap) })
                                      : undefined
                            "
                        >
                            <i
                                v-if="timeline.rows[index]?.late"
                                class="bi bi-exclamation-triangle-fill me-1 text-warning"
                            />
                            {{ clock(starts[index] ?? 0) }}
                        </span>
                        <!-- No celular o nome vai para uma linha só dele; no computador fica entre a hora e a duração. -->
                        <input
                            v-if="canManage"
                            v-model="item.title"
                            class="input input-xs order-last w-full min-w-0 sm:order-none sm:w-auto sm:grow"
                            maxlength="100"
                        />
                        <span v-else class="order-last w-full truncate sm:order-none sm:w-auto sm:grow">
                            {{ item.title }}
                        </span>
                        <span class="ms-auto w-16 text-right tabular-nums opacity-70 sm:ms-0">
                            {{ formatDuration(item.duration) }}
                        </span>
                        <template v-if="canManage">
                            <!-- Computador: hora marcada, subir, descer e tirar, na própria linha. -->
                            <span class="hidden items-center gap-1 sm:flex">
                                <span v-if="item.fixed" class="flex items-center gap-1">
                                    <input
                                        v-model="item.fixed"
                                        type="time"
                                        class="input input-xs w-24"
                                        :aria-label="t('commercial.grid.fixedTime')"
                                    />
                                    <button
                                        class="btn btn-xs btn-ghost"
                                        :title="t('commercial.grid.fixedClear')"
                                        :aria-label="t('commercial.grid.fixedClear')"
                                        @click="item.fixed = ''"
                                    >
                                        <i class="bi bi-alarm-fill text-accent" />
                                    </button>
                                </span>
                                <button
                                    v-else
                                    class="btn btn-xs btn-ghost"
                                    :title="t('commercial.grid.fixedHelp')"
                                    :aria-label="t('commercial.grid.fixedAdd')"
                                    @click="item.fixed = clock(starts[index] ?? 0).slice(0, 5)"
                                >
                                    <i class="bi bi-alarm" />
                                </button>
                                <button
                                    class="btn btn-xs btn-ghost"
                                    :disabled="index === 0"
                                    :title="t('commercial.grid.up')"
                                    @click="move(index, -1)"
                                >
                                    <i class="bi bi-arrow-up" />
                                </button>
                                <button
                                    class="btn btn-xs btn-ghost"
                                    :disabled="index === items.length - 1"
                                    :title="t('commercial.grid.down')"
                                    @click="move(index, 1)"
                                >
                                    <i class="bi bi-arrow-down" />
                                </button>
                                <button
                                    class="btn btn-xs btn-ghost"
                                    :title="t('commercial.remove')"
                                    @click="remove(index)"
                                >
                                    <i class="bi bi-x-lg" />
                                </button>
                            </span>
                            <!-- Celular: as mesmas ações numa folha que sobe de baixo. -->
                            <button
                                class="btn btn-ghost btn-square btn-sm sm:hidden"
                                :aria-label="t('commercial.grid.actions')"
                                @click="sheetFor = index"
                            >
                                <i class="bi bi-three-dots-vertical text-lg" />
                            </button>
                            <span v-if="item.fixed" class="order-last flex w-full items-center gap-2 sm:hidden">
                                <i class="bi bi-alarm-fill text-accent" />
                                <span class="text-sm opacity-70">{{ t('commercial.grid.fixedTime') }}</span>
                                <input
                                    v-model="item.fixed"
                                    type="time"
                                    class="input input-sm w-32"
                                    :aria-label="t('commercial.grid.fixedTime')"
                                />
                                <button
                                    class="btn btn-ghost btn-square btn-sm"
                                    :aria-label="t('commercial.grid.fixedClear')"
                                    @click="item.fixed = ''"
                                >
                                    <i class="bi bi-x-lg" />
                                </button>
                            </span>
                        </template>
                    </li>
                </ol>
            </div>

            <div v-if="canManage" class="mt-4 flex flex-wrap items-center gap-2">
                <button class="btn btn-sm" @click="showPicker = true">
                    <i class="bi bi-plus-lg" /> {{ t('commercial.grid.addPrograms') }}
                </button>
                <button class="btn btn-sm btn-accent" :disabled="!dirty" @click="save">
                    {{ t('commercial.grid.saveDay') }}
                </button>
                <button class="btn btn-sm" :disabled="dirty || !items.length" @click="showCopy = true">
                    {{ t('commercial.grid.copy') }}
                </button>
                <span v-if="dirty" class="text-sm text-warning">{{ t('commercial.grid.unsaved') }}</span>
            </div>
            <p class="text-xs opacity-60 mt-3">{{ t('commercial.grid.nextStep') }}</p>
        </section>

        <ActionSheet
            :show="sheetFor !== null"
            :title="sheetFor !== null ? items[sheetFor]?.title : ''"
            @close="sheetFor = null"
        >
            <template v-if="sheetFor !== null && items[sheetFor]">
                <button
                    v-if="items[sheetFor]!.fixed"
                    class="btn btn-ghost justify-start"
                    @click="items[sheetFor!]!.fixed = ''"
                >
                    <i class="bi bi-alarm-fill text-accent" /> {{ t('commercial.grid.fixedClear') }}
                </button>
                <button
                    v-else
                    class="btn btn-ghost justify-start"
                    @click="items[sheetFor!]!.fixed = clock(starts[sheetFor!] ?? 0).slice(0, 5)"
                >
                    <i class="bi bi-alarm" /> {{ t('commercial.grid.fixedAdd') }}
                </button>
                <button class="btn btn-ghost justify-start" :disabled="sheetFor === 0" @click="move(sheetFor, -1)">
                    <i class="bi bi-arrow-up" /> {{ t('commercial.grid.up') }}
                </button>
                <button
                    class="btn btn-ghost justify-start"
                    :disabled="sheetFor === items.length - 1"
                    @click="move(sheetFor, 1)"
                >
                    <i class="bi bi-arrow-down" /> {{ t('commercial.grid.down') }}
                </button>
                <button class="btn btn-ghost justify-start text-error" @click="remove(sheetFor)">
                    <i class="bi bi-x-lg" /> {{ t('commercial.remove') }}
                </button>
            </template>
        </ActionSheet>

        <StorageFilePicker :show="showPicker" @close="showPicker = false" @pick="addPicked" />
        <GenericModal
            :show="showCopy"
            :title="t('commercial.grid.copyTitle')"
            :text="t('commercial.grid.copyText', { day: t(`commercial.weekday.${weekday}`) })"
            :modal-action="copyToOthers"
        />
        <GenericModal
            :show="pendingDay !== null"
            :title="t('commercial.grid.discardTitle')"
            :text="t('commercial.grid.discardText')"
            :modal-action="answerDiscard"
        />
    </div>
</template>
