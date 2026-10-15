<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useHead } from '@unhead/vue'

import CgEditor from '@/components/cg/CgEditor.vue'
import ConfirmModal from '@/components/ui/ConfirmModal.vue'
import EmptyState from '@/components/ui/EmptyState.vue'
import HelpTip from '@/components/ui/HelpTip.vue'
import PageHeader from '@/components/ui/PageHeader.vue'
import StatusPill from '@/components/ui/StatusPill.vue'

import { friendlyError } from '@/composables/friendlyError'
import { shortDate, useCommercialApi } from '@/composables/commercialApi'
import { ALL_DAYS, WORK_DAYS, newCg, shortClock, useCgApi, type Cg, type CgRule } from '@/composables/cgApi'
import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'

const REFRESH_MS = 5000
const WEEKEND = 96
const SHOW_CHOICES = [10, 15, 30, 60]

const { t } = useI18n()
const api = useCgApi()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

useHead({ title: computed(() => t('nav.cg')) })

const items = ref<Cg[]>([])
const onAir = ref<number[]>([])
const loaded = ref(false)
const editing = ref<Cg | null>(null)
const saving = ref(false)
const removing = ref<Cg | null>(null)
const showSeconds = ref(15)
const campaigns = ref<Record<number, string>>({})

const canManage = computed(() => authStore.role !== 'user')

let timer: ReturnType<typeof setInterval> | undefined

function fail(error: unknown) {
    indexStore.msgAlert('error', friendlyError(error), 6)
}

async function load() {
    try {
        const overview = await api.overview()

        if (overview.items.some((item) => item.campaign_id !== null)) {
            const all = await useCommercialApi().campaigns(true)

            campaigns.value = Object.fromEntries(all.map((campaign) => [campaign.id, campaign.name]))
        }

        items.value = overview.items
        onAir.value = overview.on_air
    } catch (error) {
        if (!loaded.value) {
            fail(error)
        }
    } finally {
        loaded.value = true
    }
}

function daysText(weekdays: number) {
    if (weekdays === ALL_DAYS) {
        return t('cg.summary.everyDay')
    }

    if (weekdays === WORK_DAYS) {
        return t('cg.summary.workDays')
    }

    if (weekdays === WEEKEND) {
        return t('cg.summary.weekend')
    }

    return [1, 2, 3, 4, 5, 6, 7]
        .filter((day) => weekdays & (1 << (day - 1)))
        .map((day) => t(`cg.weekday.${day}`))
        .join(', ')
}

function periodText(rule: CgRule) {
    const from = shortDate(rule.date_from)
    const to = shortDate(rule.date_to)

    if (from && to) {
        return t('cg.summary.period', { from, to })
    }

    if (from) {
        return t('cg.summary.periodFrom', { from })
    }

    return to ? t('cg.summary.periodTo', { to }) : ''
}

/** One plain sentence per rule, so the list reads without opening the editor. */
function describe(rule: CgRule) {
    const days = daysText(rule.weekdays)

    if (rule.mode === 'program') {
        const offset = rule.offset_seconds
            ? t('cg.summary.afterSeconds', { n: rule.offset_seconds })
            : t('cg.summary.immediately')
        const key = rule.program_match.trim() ? 'programNamed' : 'programAny'

        return (
            t(`cg.summary.${key}`, {
                days,
                offset,
                name: rule.program_match.trim(),
                seconds: rule.duration_seconds,
            }) + periodText(rule)
        )
    }

    let text = t('cg.summary.clock', { days, time: shortClock(rule.start_time), seconds: rule.duration_seconds })

    if (rule.repeat_every_seconds) {
        const until = rule.until_time ? t('cg.summary.until', { time: shortClock(rule.until_time) }) : ''

        text += t('cg.summary.repeat', { minutes: Math.round(rule.repeat_every_seconds / 60), until })
    }

    return text + periodText(rule)
}

async function save(item: Cg) {
    saving.value = true

    try {
        await api.save(item)
        editing.value = null
        indexStore.msgAlert('success', t('cg.saved'), 3)
        await load()
    } catch (error) {
        fail(error)
    } finally {
        saving.value = false
    }
}

async function toggle(item: Cg) {
    try {
        await api.save({ ...item, enabled: !item.enabled })
        await load()
    } catch (error) {
        fail(error)
    }
}

async function remove(confirmed: boolean) {
    const item = removing.value

    removing.value = null

    if (!confirmed || !item?.id) {
        return
    }

    try {
        await api.remove(item.id)
        indexStore.msgAlert('success', t('cg.removed'), 3)
        await load()
    } catch (error) {
        fail(error)
    }
}

async function show(item: Cg) {
    try {
        await api.show(item.id as number, showSeconds.value)
        indexStore.msgAlert('success', t('cg.shown'), 3)
        await load()
    } catch (error) {
        fail(error)
    }
}

async function hide(item: Cg) {
    try {
        await api.hide(item.id as number)
        indexStore.msgAlert('success', t('cg.hidden'), 3)
        await load()
    } catch (error) {
        fail(error)
    }
}

onMounted(() => {
    void load()
    timer = setInterval(() => {
        if (!editing.value) {
            void load()
        }
    }, REFRESH_MS)
})
onBeforeUnmount(() => clearInterval(timer))
watch(() => configStore.i, load)
</script>
<template>
    <div class="max-w-5xl mx-auto p-4">
        <CgEditor v-if="editing" :cg="editing" :saving="saving" @save="save" @cancel="editing = null" />

        <template v-else>
            <PageHeader :title="t('cg.title')" :subtitle="t('cg.subtitle')">
                <template #actions>
                    <HelpTip :text="t('cg.helpTip')" />
                    <label class="flex items-center gap-2 text-sm">
                        {{ t('cg.showNow') }} {{ t('cg.showFor') }}
                        <select v-model.number="showSeconds" class="select select-sm w-24">
                            <option v-for="seconds in SHOW_CHOICES" :key="seconds" :value="seconds">
                                {{ t('cg.seconds', { n: seconds }) }}
                            </option>
                        </select>
                    </label>
                    <button v-if="canManage" class="btn btn-accent" @click="editing = newCg()">
                        <i class="bi bi-plus-lg" /> {{ t('cg.add') }}
                    </button>
                </template>
            </PageHeader>

            <EmptyState
                v-if="loaded && !items.length"
                icon="bi-layers"
                :title="t('cg.emptyTitle')"
                :text="canManage ? t('cg.emptyText') : t('cg.emptyReadOnly')"
            >
                <button v-if="canManage" class="btn btn-accent" @click="editing = newCg()">
                    <i class="bi bi-plus-lg" /> {{ t('cg.add') }}
                </button>
            </EmptyState>

            <ul class="space-y-3">
                <li
                    v-for="item in items"
                    :key="item.id"
                    class="card bg-base-100 shadow p-4"
                    :class="!item.enabled && 'opacity-60'"
                >
                    <div class="flex flex-wrap items-start gap-3">
                        <i class="bi text-3xl opacity-60" :class="item.kind === 'text' ? 'bi-fonts' : 'bi-image'" />
                        <div class="grow min-w-0">
                            <div class="flex flex-wrap items-center gap-2">
                                <h2 class="font-bold text-lg truncate">{{ item.name }}</h2>
                                <StatusPill
                                    v-if="onAir.includes(item.id as number)"
                                    kind="live"
                                    :label="t('cg.onAir')"
                                />
                                <StatusPill v-else-if="!item.enabled" kind="idle" :label="t('cg.off')" />
                            </div>
                            <p class="text-sm opacity-70 truncate">
                                {{ item.kind === 'text' ? item.text : item.image_path }}
                            </p>
                            <p v-if="item.campaign_id !== null" class="text-sm">
                                <i class="bi bi-megaphone me-1 opacity-60" />
                                {{ t('cg.campaignOf', { name: campaigns[item.campaign_id] ?? item.campaign_id }) }}
                            </p>
                            <ul class="mt-2 space-y-1 text-sm">
                                <li v-if="!item.schedules.length" class="opacity-60">{{ t('cg.noRules') }}</li>
                                <li v-for="(rule, index) in item.schedules" :key="index">
                                    <i class="bi bi-clock me-1 opacity-60" />{{ describe(rule) }}
                                </li>
                            </ul>
                        </div>
                        <div class="flex flex-wrap items-center gap-2">
                            <button
                                v-if="onAir.includes(item.id as number)"
                                class="btn btn-sm btn-warning"
                                @click="hide(item)"
                            >
                                <i class="bi bi-eye-slash" /> {{ t('cg.takeOff') }}
                            </button>
                            <button v-else class="btn btn-sm btn-accent" :disabled="!item.enabled" @click="show(item)">
                                <i class="bi bi-eye" /> {{ t('cg.showNow') }}
                            </button>
                            <template v-if="canManage">
                                <input
                                    type="checkbox"
                                    class="toggle toggle-sm toggle-accent"
                                    :checked="item.enabled"
                                    :title="t('cg.enabledHelp')"
                                    :aria-label="t('cg.enabledHelp')"
                                    @change="toggle(item)"
                                />
                                <button class="btn btn-sm" @click="editing = item">{{ t('cg.edit') }}</button>
                                <button
                                    class="btn btn-sm btn-ghost"
                                    :title="t('cg.remove')"
                                    :aria-label="t('cg.remove')"
                                    @click="removing = item"
                                >
                                    <i class="bi bi-trash" />
                                </button>
                            </template>
                        </div>
                    </div>
                </li>
            </ul>
        </template>

        <ConfirmModal
            :show="removing !== null"
            :title="t('cg.removeTitle')"
            :text="t('cg.removeText', { name: removing?.name ?? '' })"
            :confirm-label="t('cg.remove')"
            danger
            @confirm="remove(true)"
            @cancel="remove(false)"
        />
    </div>
</template>
