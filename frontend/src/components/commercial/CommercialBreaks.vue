<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import GenericModal from '@/components/utils/GenericModal.vue'
import WeekdayTabs from '@/components/commercial/WeekdayTabs.vue'

import { friendlyError } from '@/composables/friendlyError'
import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'
import { formatDuration, WEEKDAYS, useCommercialApi } from '@/composables/commercialApi'

const { t } = useI18n()
const api = useCommercialApi()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const canManage = computed(() => authStore.role !== 'user')
const weekday = ref(new Date().getDay() || 7)
const windows = ref<CommercialBreakWindow[]>([])
const form = ref<Partial<CommercialBreakWindow> | null>(null)
const removing = ref<CommercialBreakWindow | null>(null)
const showCopy = ref(false)

const dayWindows = computed(() => windows.value.filter((window) => window.weekday === weekday.value))
const totalSec = computed(() =>
    dayWindows.value.filter((window) => window.enabled).reduce((sum, window) => sum + window.duration_sec, 0),
)

function fail(error: unknown) {
    indexStore.msgAlert('error', friendlyError(error), 6)
}

async function load() {
    try {
        windows.value = await api.breakWindows()
    } catch (error) {
        fail(error)
    }
}

function startForm(window: CommercialBreakWindow | null) {
    form.value = window
        ? { ...window }
        : { weekday: weekday.value, nominal_time: '12:00:00', duration_sec: 120, tolerance_sec: 60, enabled: true }
}

async function save() {
    if (!form.value) {
        return
    }

    try {
        await api.saveBreakWindow({ ...form.value, weekday: weekday.value })
        form.value = null
        indexStore.msgAlert('success', t('commercial.saved'), 3)
        await load()
    } catch (error) {
        fail(error)
    }
}

async function toggle(window: CommercialBreakWindow) {
    try {
        await api.saveBreakWindow({ ...window, enabled: !window.enabled })
        await load()
    } catch (error) {
        fail(error)
    }
}

async function remove(confirmed: boolean) {
    const window = removing.value

    removing.value = null

    if (!confirmed || !window) {
        return
    }

    try {
        await api.removeBreakWindow(window.id)
        await load()
    } catch (error) {
        fail(error)
    }
}

/** Replace the windows of every other day with the ones shown. */
async function copyToOthers(confirmed: boolean) {
    showCopy.value = false

    if (!confirmed) {
        return
    }

    try {
        const source = [...dayWindows.value]

        for (const day of WEEKDAYS.filter((value) => value !== weekday.value)) {
            for (const old of windows.value.filter((window) => window.weekday === day)) {
                await api.removeBreakWindow(old.id)
            }

            for (const window of source) {
                await api.saveBreakWindow({
                    weekday: day,
                    nominal_time: window.nominal_time,
                    duration_sec: window.duration_sec,
                    tolerance_sec: window.tolerance_sec,
                    enabled: window.enabled,
                })
            }
        }

        indexStore.msgAlert('success', t('commercial.breaks.copied'), 4)
        await load()
    } catch (error) {
        fail(error)
        await load()
    }
}

onMounted(load)
watch(() => configStore.i, load)
</script>
<template>
    <div class="space-y-4">
        <p class="text-sm opacity-80">{{ t('commercial.breaks.help') }}</p>
        <WeekdayTabs v-model="weekday" />

        <section class="card bg-base-100 shadow p-4">
            <div class="flex items-center gap-2 mb-2">
                <h2 class="font-bold text-lg grow">{{ t(`commercial.weekday.${weekday}`) }}</h2>
                <span class="text-sm opacity-70">
                    {{ t('commercial.breaks.total', { time: formatDuration(totalSec) }) }}
                </span>
            </div>

            <p v-if="!dayWindows.length" class="opacity-70">{{ t('commercial.breaks.empty') }}</p>
            <div v-else class="overflow-x-auto">
                <table class="table table-sm">
                    <thead>
                        <tr>
                            <th>{{ t('commercial.breaks.time') }}</th>
                            <th>{{ t('commercial.breaks.length') }}</th>
                            <th>{{ t('commercial.breaks.tolerance') }}</th>
                            <th>{{ t('commercial.breaks.active') }}</th>
                            <th />
                        </tr>
                    </thead>
                    <tbody>
                        <tr v-for="window in dayWindows" :key="window.id" :class="!window.enabled && 'opacity-50'">
                            <td class="tabular-nums">{{ window.nominal_time }}</td>
                            <td>{{ formatDuration(window.duration_sec) }}</td>
                            <td>{{ window.tolerance_sec }} s</td>
                            <td>
                                <input
                                    type="checkbox"
                                    class="toggle toggle-sm toggle-accent"
                                    :checked="window.enabled"
                                    :disabled="!canManage"
                                    @change="toggle(window)"
                                />
                            </td>
                            <td class="text-right whitespace-nowrap">
                                <template v-if="canManage">
                                    <button class="btn btn-xs" @click="startForm(window)">
                                        {{ t('commercial.edit') }}
                                    </button>
                                    <button
                                        class="btn btn-xs btn-ghost"
                                        :title="t('commercial.remove')"
                                        @click="removing = window"
                                    >
                                        <i class="bi bi-trash" />
                                    </button>
                                </template>
                            </td>
                        </tr>
                    </tbody>
                </table>
            </div>

            <div v-if="form" class="mt-4 grid gap-3 sm:grid-cols-3 items-end">
                <label class="form-control">
                    <span class="label-text mb-1">{{ t('commercial.breaks.time') }}</span>
                    <input v-model="form.nominal_time" type="time" step="1" class="input input-sm w-full" />
                </label>
                <label class="form-control">
                    <span class="label-text mb-1">{{ t('commercial.breaks.lengthSeconds') }}</span>
                    <input v-model.number="form.duration_sec" type="number" min="1" class="input input-sm w-full" />
                </label>
                <label class="form-control">
                    <span class="label-text mb-1">{{ t('commercial.breaks.tolerance') }}</span>
                    <input v-model.number="form.tolerance_sec" type="number" min="0" class="input input-sm w-full" />
                </label>
                <p class="sm:col-span-3 text-xs opacity-60">{{ t('commercial.breaks.toleranceHelp') }}</p>
                <div class="sm:col-span-3 flex justify-end gap-2">
                    <button class="btn btn-sm" @click="form = null">{{ t('cancel') }}</button>
                    <button class="btn btn-sm btn-accent" @click="save">{{ t('commercial.save') }}</button>
                </div>
            </div>
            <div v-else-if="canManage" class="mt-4 flex flex-wrap gap-2">
                <button class="btn btn-sm btn-accent" @click="startForm(null)">
                    <i class="bi bi-plus-lg" /> {{ t('commercial.breaks.new') }}
                </button>
                <button class="btn btn-sm" :disabled="!dayWindows.length" @click="showCopy = true">
                    {{ t('commercial.breaks.copy') }}
                </button>
            </div>
        </section>

        <GenericModal
            :show="removing !== null"
            :title="t('commercial.breaks.removeTitle')"
            :text="t('commercial.breaks.removeText', { time: removing?.nominal_time ?? '' })"
            :modal-action="remove"
        />
        <GenericModal
            :show="showCopy"
            :title="t('commercial.breaks.copyTitle')"
            :text="t('commercial.breaks.copyText', { day: t(`commercial.weekday.${weekday}`) })"
            :modal-action="copyToOthers"
        />
    </div>
</template>
