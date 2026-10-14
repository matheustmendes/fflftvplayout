<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import GenericModal from '@/components/utils/GenericModal.vue'
import StorageFilePicker from '@/components/commercial/StorageFilePicker.vue'
import WeekdayTabs from '@/components/commercial/WeekdayTabs.vue'
import type { PickedFile } from '@/components/commercial/StorageFilePicker.vue'

import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'
import { formatDuration, WEEKDAYS, useCommercialApi } from '@/composables/commercialApi'

interface GridItem {
    source: string
    title: string
    duration: number
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

const dirty = computed(() => JSON.stringify(items.value) !== saved.value)
const totalSec = computed(() => items.value.reduce((sum, item) => sum + item.duration, 0))
const dayLength = computed(() => configStore.playlistLength || DAY_SECONDS)
const overflow = computed(() => totalSec.value - dayLength.value)

function fail(error: unknown) {
    indexStore.msgAlert('error', error instanceof Error ? error.message : String(error), 5)
}

async function load() {
    try {
        const rows = await api.weekDay(weekday.value)

        items.value = rows.map((row) => ({
            source: row.source,
            title: row.title,
            duration: row.out_sec - row.in_sec,
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
        })),
    ]
}

const payload = () => items.value.map((item) => ({ source: item.source, title: item.title }))

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

            <p v-if="!items.length" class="opacity-70">{{ t('commercial.grid.empty') }}</p>
            <ol v-else class="divide-y divide-base-300 text-sm">
                <li
                    v-for="(item, index) in items"
                    :key="`${index}-${item.source}`"
                    class="flex items-center gap-2 py-1.5"
                >
                    <span class="w-6 text-right opacity-60">{{ index + 1 }}</span>
                    <input v-if="canManage" v-model="item.title" class="input input-xs grow min-w-0" maxlength="100" />
                    <span v-else class="grow truncate">{{ item.title }}</span>
                    <span class="hidden md:block text-xs opacity-50 truncate max-w-56">{{ item.source }}</span>
                    <span class="w-16 text-right tabular-nums opacity-70">{{ formatDuration(item.duration) }}</span>
                    <template v-if="canManage">
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
                        <button class="btn btn-xs btn-ghost" :title="t('commercial.remove')" @click="remove(index)">
                            <i class="bi bi-x-lg" />
                        </button>
                    </template>
                </li>
            </ol>

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
