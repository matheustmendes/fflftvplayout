<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import CgStage from '@/components/cg/CgStage.vue'
import StorageFilePicker, { type PickedFile } from '@/components/commercial/StorageFilePicker.vue'
import HelpTip from '@/components/ui/HelpTip.vue'

import { useCommercialApi } from '@/composables/commercialApi'
import { ALL_DAYS, WORK_DAYS, newRule, shortClock, type Cg, type CgRule } from '@/composables/cgApi'

const IMAGE_EXTENSIONS = ['png', 'jpg', 'jpeg', 'webp', 'bmp']
const WEEKEND = 96
const DAY_NUMBERS = [1, 2, 3, 4, 5, 6, 7]

const props = defineProps<{ cg: Cg; saving: boolean }>()
const emit = defineEmits<{
    (event: 'save', cg: Cg): void
    (event: 'cancel'): void
}>()

const { t } = useI18n()

const draft = ref<Cg>({
    ...props.cg,
    schedules: props.cg.schedules.map((rule) => ({
        ...rule,
        start_time: shortClock(rule.start_time),
        until_time: rule.until_time ? shortClock(rule.until_time) : null,
    })),
})
const picking = ref(false)
const campaigns = ref<CommercialCampaign[]>([])

onMounted(async () => {
    try {
        campaigns.value = (await useCommercialApi().campaigns()).filter(
            (campaign) => campaign.active || campaign.id === draft.value.campaign_id,
        )
    } catch {
        campaigns.value = []
    }
})

const isNew = computed(() => !draft.value.id)
const canSave = computed(
    () =>
        draft.value.name.trim() !== '' &&
        (draft.value.kind === 'text' ? draft.value.text.trim() !== '' : draft.value.image_path !== ''),
)

const DEFAULT_TEXT_SIZE = 5
const DEFAULT_IMAGE_SIZE = 12

/** Switching the type also moves the size away from the other type's default. */
function setKind(kind: Cg['kind']) {
    const untouched = draft.value.kind === 'text' ? DEFAULT_TEXT_SIZE : DEFAULT_IMAGE_SIZE

    if (draft.value.size_pct === untouched) {
        draft.value.size_pct = kind === 'text' ? DEFAULT_TEXT_SIZE : DEFAULT_IMAGE_SIZE
    }

    draft.value.kind = kind

    if (kind === 'image') {
        draft.value.scroll = 'none'
    }
}

const dayBit = (day: number) => 1 << (day - 1)

function toggleDay(rule: CgRule, day: number) {
    const next = rule.weekdays ^ dayBit(day)

    // A rule with no day would never run; keep at least one.
    rule.weekdays = next === 0 ? rule.weekdays : next
}

function repeatOn(rule: CgRule) {
    return rule.repeat_every_seconds !== null
}

function toggleRepeat(rule: CgRule, on: boolean) {
    rule.repeat_every_seconds = on ? Math.max(rule.duration_seconds, 1800) : null
    rule.until_time = on ? '23:00' : null
}

function repeatMinutes(rule: CgRule) {
    return Math.round((rule.repeat_every_seconds ?? 0) / 60)
}

function setRepeatMinutes(rule: CgRule, minutes: number) {
    rule.repeat_every_seconds = Math.max(1, Math.round(minutes || 1)) * 60
}

function setMode(rule: CgRule, mode: CgRule['mode']) {
    rule.mode = mode

    if (mode === 'program') {
        rule.repeat_every_seconds = null
        rule.until_time = null
    }
}

function move(x: number, y: number) {
    draft.value.x_pct = x
    draft.value.y_pct = y
}

function pickImage(files: PickedFile[]) {
    picking.value = false

    if (files[0]) {
        draft.value.image_path = files[0].path
    }
}

const percent = computed({
    get: () => Math.round(draft.value.opacity * 100),
    set: (value: number) => (draft.value.opacity = Math.min(100, Math.max(0, value)) / 100),
})
const boxPercent = computed({
    get: () => Math.round(draft.value.box_opacity * 100),
    set: (value: number) => (draft.value.box_opacity = Math.min(100, Math.max(0, value)) / 100),
})

function submit() {
    emit('save', {
        ...draft.value,
        name: draft.value.name.trim(),
        schedules: draft.value.schedules.map((rule) => ({
            ...rule,
            date_from: rule.date_from || null,
            date_to: rule.date_to || null,
        })),
    })
}
</script>
<template>
    <form class="space-y-5" @submit.prevent="submit">
        <div class="flex items-center gap-3">
            <button type="button" class="btn btn-ghost btn-sm" @click="emit('cancel')">
                <i class="bi bi-arrow-left" /> {{ t('cg.editor.back') }}
            </button>
            <h2 class="text-xl font-bold grow">{{ isNew ? t('cg.editor.newTitle') : t('cg.editor.editTitle') }}</h2>
        </div>

        <div class="grid gap-5 lg:grid-cols-2">
            <div class="space-y-5">
                <section class="card bg-base-100 shadow p-4 space-y-3">
                    <h3 class="font-bold">{{ t('cg.editor.stepWhat') }}</h3>

                    <label class="form-control">
                        <span class="label-text mb-1">{{ t('cg.editor.name') }}</span>
                        <input
                            v-model="draft.name"
                            class="input w-full"
                            maxlength="120"
                            :placeholder="t('cg.editor.namePlaceholder')"
                        />
                    </label>

                    <label v-if="campaigns.length" class="form-control">
                        <span class="label-text mb-1">
                            {{ t('cg.editor.campaign') }} <HelpTip :text="t('cg.editor.campaignHelp')" />
                        </span>
                        <select v-model="draft.campaign_id" class="select select-sm w-full">
                            <option :value="null">{{ t('cg.editor.campaignNone') }}</option>
                            <option v-for="campaign in campaigns" :key="campaign.id" :value="campaign.id">
                                {{ campaign.name }}
                            </option>
                        </select>
                    </label>

                    <div role="tablist" class="tabs tabs-box tabs-sm w-fit">
                        <button
                            type="button"
                            role="tab"
                            class="tab"
                            :class="draft.kind === 'text' && 'tab-active'"
                            @click="setKind('text')"
                        >
                            <i class="bi bi-fonts me-1" />{{ t('cg.kindText') }}
                        </button>
                        <button
                            type="button"
                            role="tab"
                            class="tab"
                            :class="draft.kind === 'image' && 'tab-active'"
                            @click="setKind('image')"
                        >
                            <i class="bi bi-image me-1" />{{ t('cg.kindImage') }}
                        </button>
                    </div>

                    <label v-if="draft.kind === 'text'" class="form-control">
                        <span class="label-text mb-1">{{ t('cg.editor.text') }}</span>
                        <textarea
                            v-model="draft.text"
                            rows="2"
                            maxlength="300"
                            class="textarea w-full"
                            :placeholder="t('cg.editor.textPlaceholder')"
                        />
                    </label>
                    <div v-else class="flex flex-wrap items-center gap-3">
                        <button type="button" class="btn btn-sm" @click="picking = true">
                            <i class="bi bi-folder2-open" />
                            {{ draft.image_path ? t('cg.editor.changeImage') : t('cg.editor.pickImage') }}
                        </button>
                        <span class="text-sm truncate min-w-0" :class="!draft.image_path && 'opacity-60'">
                            {{ draft.image_path || t('cg.editor.noImage') }}
                        </span>
                    </div>
                </section>

                <section class="card bg-base-100 shadow p-4 space-y-3">
                    <h3 class="font-bold">{{ t('cg.editor.stepLook') }}</h3>

                    <label class="form-control">
                        <span class="label-text mb-1">
                            {{ t('cg.editor.size') }}: {{ draft.size_pct }}%
                            <HelpTip
                                :text="
                                    draft.kind === 'text' ? t('cg.editor.sizeTextHelp') : t('cg.editor.sizeImageHelp')
                                "
                            />
                        </span>
                        <input
                            v-model.number="draft.size_pct"
                            type="range"
                            min="1"
                            max="100"
                            step="0.5"
                            class="range range-sm range-accent"
                        />
                    </label>

                    <template v-if="draft.kind === 'text'">
                        <div class="grid gap-3 sm:grid-cols-2">
                            <label class="form-control">
                                <span class="label-text mb-1">{{ t('cg.editor.color') }}</span>
                                <input v-model="draft.text_color" type="color" class="input input-sm w-full p-1" />
                            </label>
                            <label class="form-control">
                                <span class="label-text mb-1">{{ t('cg.editor.weight') }}</span>
                                <select v-model="draft.font_weight" class="select select-sm w-full">
                                    <option value="normal">{{ t('cg.editor.weightNormal') }}</option>
                                    <option value="semibold">{{ t('cg.editor.weightSemibold') }}</option>
                                    <option value="bold">{{ t('cg.editor.weightBold') }}</option>
                                </select>
                            </label>
                        </div>

                        <label class="flex items-center gap-2 cursor-pointer">
                            <input v-model="draft.box_enabled" type="checkbox" class="toggle toggle-sm toggle-accent" />
                            <span>{{ t('cg.editor.box') }}</span>
                        </label>
                        <div v-if="draft.box_enabled" class="grid gap-3 sm:grid-cols-2">
                            <label class="form-control">
                                <span class="label-text mb-1">{{ t('cg.editor.boxColor') }}</span>
                                <input v-model="draft.box_color" type="color" class="input input-sm w-full p-1" />
                            </label>
                            <label class="form-control">
                                <span class="label-text mb-1">{{ t('cg.editor.boxOpacity') }}: {{ boxPercent }}%</span>
                                <input
                                    v-model.number="boxPercent"
                                    type="range"
                                    min="0"
                                    max="100"
                                    class="range range-sm range-accent"
                                />
                            </label>
                        </div>

                        <label class="form-control">
                            <span class="label-text mb-1">
                                {{ t('cg.editor.scroll') }} <HelpTip :text="t('cg.editor.scrollHelp')" />
                            </span>
                            <select v-model="draft.scroll" class="select select-sm w-full">
                                <option value="none">{{ t('cg.editor.scrollNone') }}</option>
                                <option value="right_to_left">{{ t('cg.editor.scrollLeft') }}</option>
                                <option value="left_to_right">{{ t('cg.editor.scrollRight') }}</option>
                            </select>
                        </label>
                        <label v-if="draft.scroll !== 'none'" class="form-control">
                            <span class="label-text mb-1">
                                {{ t('cg.editor.scrollSpeed') }}: {{ draft.scroll_speed_pct }}%
                            </span>
                            <input
                                v-model.number="draft.scroll_speed_pct"
                                type="range"
                                min="2"
                                max="40"
                                step="1"
                                class="range range-sm range-accent"
                            />
                        </label>

                        <label class="form-control">
                            <span class="label-text mb-1">
                                {{ t('cg.editor.font') }} <HelpTip :text="t('cg.editor.fontHelp')" />
                            </span>
                            <input v-model="draft.font_family" class="input input-sm w-full" maxlength="80" />
                        </label>
                    </template>

                    <label class="form-control">
                        <span class="label-text mb-1">
                            {{ t('cg.editor.opacity') }}: {{ percent }}% <HelpTip :text="t('cg.editor.opacityHelp')" />
                        </span>
                        <input
                            v-model.number="percent"
                            type="range"
                            min="10"
                            max="100"
                            class="range range-sm range-accent"
                        />
                    </label>

                    <label class="form-control">
                        <span class="label-text mb-1">
                            {{ t('cg.editor.fadeIn') }} <HelpTip :text="t('cg.editor.fadeInHelp')" />
                        </span>
                        <input
                            v-model.number="draft.fade_in_seconds"
                            type="number"
                            min="0"
                            max="10"
                            step="0.5"
                            class="input input-sm w-32"
                        />
                    </label>

                    <label class="form-control">
                        <span class="label-text mb-1">
                            {{ t('cg.editor.fadeOut') }} <HelpTip :text="t('cg.editor.fadeOutHelp')" />
                        </span>
                        <input
                            v-model.number="draft.fade_out_seconds"
                            type="number"
                            min="0"
                            max="10"
                            step="0.5"
                            class="input input-sm w-32"
                        />
                    </label>
                </section>
            </div>

            <section class="card bg-base-100 shadow p-4 space-y-3 self-start lg:sticky lg:top-4">
                <h3 class="font-bold">{{ t('cg.editor.stepWhere') }}</h3>
                <p class="text-sm opacity-70">{{ t('cg.editor.stageHelp') }}</p>
                <CgStage :cg="draft" @move="move" />
                <div class="grid grid-cols-2 gap-3">
                    <label class="form-control">
                        <span class="label-text mb-1">{{ t('cg.editor.left') }}</span>
                        <input
                            v-model.number="draft.x_pct"
                            type="number"
                            min="0"
                            max="100"
                            step="0.1"
                            class="input input-sm w-full"
                        />
                    </label>
                    <label class="form-control">
                        <span class="label-text mb-1">{{ t('cg.editor.top') }}</span>
                        <input
                            v-model.number="draft.y_pct"
                            type="number"
                            min="0"
                            max="100"
                            step="0.1"
                            class="input input-sm w-full"
                        />
                    </label>
                </div>
            </section>
        </div>

        <section class="card bg-base-100 shadow p-4 space-y-4">
            <div class="flex items-center gap-2">
                <h3 class="font-bold grow">{{ t('cg.editor.stepWhen') }}</h3>
                <button type="button" class="btn btn-sm" @click="draft.schedules.push(newRule())">
                    <i class="bi bi-plus-lg" /> {{ t('cg.editor.addRule') }}
                </button>
            </div>
            <p class="text-sm opacity-70">{{ t('cg.editor.rulesHelp') }}</p>
            <p v-if="!draft.schedules.length" class="text-sm opacity-70">{{ t('cg.noRules') }}</p>

            <div
                v-for="(rule, index) in draft.schedules"
                :key="index"
                class="rounded-lg border border-base-300 p-3 space-y-3"
            >
                <div class="flex flex-wrap items-end gap-3">
                    <label class="form-control grow min-w-48">
                        <span class="label-text mb-1">{{ t('cg.editor.mode') }}</span>
                        <select
                            class="select select-sm w-full"
                            :value="rule.mode"
                            @change="setMode(rule, ($event.target as HTMLSelectElement).value as CgRule['mode'])"
                        >
                            <option value="clock">{{ t('cg.editor.modeClock') }}</option>
                            <option value="program">{{ t('cg.editor.modeProgram') }}</option>
                        </select>
                    </label>
                    <button
                        type="button"
                        class="btn btn-sm btn-ghost"
                        :title="t('cg.editor.removeRule')"
                        :aria-label="t('cg.editor.removeRule')"
                        @click="draft.schedules.splice(index, 1)"
                    >
                        <i class="bi bi-trash" />
                    </button>
                </div>

                <div>
                    <span class="label-text mb-1 block">{{ t('cg.editor.days') }}</span>
                    <div class="flex flex-wrap items-center gap-1">
                        <button
                            v-for="day in DAY_NUMBERS"
                            :key="day"
                            type="button"
                            class="btn btn-xs"
                            :class="rule.weekdays & dayBit(day) ? 'btn-accent' : 'btn-ghost border-base-300'"
                            :aria-pressed="Boolean(rule.weekdays & dayBit(day))"
                            @click="toggleDay(rule, day)"
                        >
                            {{ t(`cg.weekday.${day}`) }}
                        </button>
                        <span class="mx-1 opacity-30">|</span>
                        <button type="button" class="btn btn-xs btn-link" @click="rule.weekdays = ALL_DAYS">
                            {{ t('cg.editor.everyDay') }}
                        </button>
                        <button type="button" class="btn btn-xs btn-link" @click="rule.weekdays = WORK_DAYS">
                            {{ t('cg.editor.workDays') }}
                        </button>
                        <button type="button" class="btn btn-xs btn-link" @click="rule.weekdays = WEEKEND">
                            {{ t('cg.editor.weekend') }}
                        </button>
                    </div>
                </div>

                <div v-if="rule.mode === 'clock'" class="space-y-3">
                    <div class="grid gap-3 sm:grid-cols-2">
                        <label class="form-control">
                            <span class="label-text mb-1">{{ t('cg.editor.at') }}</span>
                            <input v-model="rule.start_time" type="time" required class="input input-sm w-full" />
                        </label>
                        <label class="form-control">
                            <span class="label-text mb-1">{{ t('cg.editor.for') }}</span>
                            <input
                                v-model.number="rule.duration_seconds"
                                type="number"
                                min="1"
                                max="86400"
                                class="input input-sm w-full"
                            />
                        </label>
                    </div>
                    <label class="flex items-center gap-2 cursor-pointer">
                        <input
                            type="checkbox"
                            class="toggle toggle-sm toggle-accent"
                            :checked="repeatOn(rule)"
                            @change="toggleRepeat(rule, ($event.target as HTMLInputElement).checked)"
                        />
                        <span>{{ t('cg.editor.repeat') }}</span>
                    </label>
                    <div v-if="repeatOn(rule)" class="grid gap-3 sm:grid-cols-2">
                        <label class="form-control">
                            <span class="label-text mb-1">{{ t('cg.editor.repeatEvery') }}</span>
                            <input
                                type="number"
                                min="1"
                                class="input input-sm w-full"
                                :value="repeatMinutes(rule)"
                                @input="setRepeatMinutes(rule, Number(($event.target as HTMLInputElement).value))"
                            />
                        </label>
                        <label class="form-control">
                            <span class="label-text mb-1">{{ t('cg.editor.repeatUntil') }}</span>
                            <input v-model="rule.until_time" type="time" class="input input-sm w-full" />
                        </label>
                    </div>
                </div>

                <div v-else class="space-y-3">
                    <label class="form-control">
                        <span class="label-text mb-1">
                            {{ t('cg.editor.programName') }} <HelpTip :text="t('cg.editor.programHelp')" />
                        </span>
                        <input v-model="rule.program_match" class="input input-sm w-full" maxlength="120" />
                    </label>
                    <div class="grid gap-3 sm:grid-cols-2">
                        <label class="form-control">
                            <span class="label-text mb-1">{{ t('cg.editor.offset') }}</span>
                            <input
                                v-model.number="rule.offset_seconds"
                                type="number"
                                min="0"
                                class="input input-sm w-full"
                            />
                        </label>
                        <label class="form-control">
                            <span class="label-text mb-1">{{ t('cg.editor.for') }}</span>
                            <input
                                v-model.number="rule.duration_seconds"
                                type="number"
                                min="1"
                                max="86400"
                                class="input input-sm w-full"
                            />
                        </label>
                    </div>
                </div>

                <div>
                    <span class="label-text mb-1 block">{{ t('cg.editor.period') }}</span>
                    <div class="grid gap-3 sm:grid-cols-2">
                        <label class="form-control">
                            <span class="text-xs opacity-70">{{ t('cg.editor.from') }}</span>
                            <input v-model="rule.date_from" type="date" class="input input-sm w-full" />
                        </label>
                        <label class="form-control">
                            <span class="text-xs opacity-70">{{ t('cg.editor.to') }}</span>
                            <input v-model="rule.date_to" type="date" class="input input-sm w-full" />
                        </label>
                    </div>
                </div>
            </div>
        </section>

        <div class="flex items-center justify-end gap-2">
            <button type="button" class="btn" @click="emit('cancel')">{{ t('cancel') }}</button>
            <button type="submit" class="btn btn-accent" :disabled="!canSave || saving">
                {{ t('cg.editor.save') }}
            </button>
        </div>

        <StorageFilePicker
            :show="picking"
            single
            :extensions="IMAGE_EXTENSIONS"
            :title="t('cg.editor.pickTitle')"
            :confirm-label="t('cg.editor.pick')"
            @close="picking = false"
            @pick="pickImage"
        />
    </form>
</template>
