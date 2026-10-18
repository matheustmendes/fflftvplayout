<script setup lang="ts">
import { computed, ref, nextTick, onMounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { storeToRefs } from 'pinia'
import { useHead } from '@unhead/vue'

import ConfirmModal from '@/components/ui/ConfirmModal.vue'
import HelpTip from '@/components/ui/HelpTip.vue'
import PageHeader from '@/components/ui/PageHeader.vue'

import { authFetch } from '@/composables/authFetch'
import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'
import { useConfig } from '@/stores/config'

type PositionChoice = 'top' | 'middle' | 'bottom' | 'custom'
type SizeChoice = 'small' | 'medium' | 'large'

const { t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const { i } = storeToRefs(useConfig())

useHead({
    title: computed(() => t('nav.message')),
})

interface PresetName {
    name: string
    value: number
}

const defaultForm = (): TextPreset => ({
    id: 0,
    channel_id: configStore.channels[configStore.i]?.id ?? 1,
    persistent: false,
    name: '',
    text: '',
    use_filename: false,
    font_family: 'DejaVu Sans',
    font_weight: 'normal',
    filename_regex: '^.+[/\\\\](.*)(.mp4|.mkv|.webm)$',
    position_x: 'center',
    position_y: 'end:72',
    font_size: 24,
    line_spacing: 4,
    text_color: '#ffffff',
    text_opacity: 1,
    background_enabled: true,
    background_color: '#000000',
    background_opacity: 0.8,
    background_padding: 4,
    opacity: 1,
    scroll_direction: 'none',
    scroll_speed: 100,
    scroll_repeat: -1,
    fade_in_seconds: 0,
    fade_out_seconds: 0,
})
const form = ref<TextPreset>(defaultForm())

// Simple-mode choices write these exact values into the same fields the advanced block edits.
const POSITION_VALUES: Record<Exclude<PositionChoice, 'custom'>, string> = {
    top: '40',
    middle: 'center',
    bottom: 'end:72',
}
const SIZE_VALUES: Record<SizeChoice, number> = { small: 18, medium: 24, large: 36 }
const COLOR_CHOICES = [
    { value: '#ffffff', key: 'white' },
    { value: '#ffd400', key: 'yellow' },
    { value: '#e11d2e', key: 'red' },
    { value: '#2563eb', key: 'blue' },
    { value: '#000000', key: 'black' },
]
// Reference frame used by the preview: the same pixel numbers the server receives.
const REFERENCE_WIDTH = 1280

const showCreateModal = ref(false)
const showDeleteModal = ref(false)
const showAdvanced = ref(false)
const selected = ref<string | null>(null)
const newPresetName = ref('')
const newPresetError = ref('')
const newPresetInput = ref<HTMLInputElement | null>(null)
const presets = ref([] as PresetName[])
const fontFamilies = ref<string[]>([])
const sending = ref(false)
const onScreenText = ref<string | null>(null)

const hasContent = computed(() => form.value.text.trim() !== '' || form.value.use_filename)

const positionChoice = computed<PositionChoice>({
    get() {
        const y = String(form.value.position_y).trim()

        if (y === 'center') {
            return 'middle'
        }

        if (y.startsWith('end:')) {
            return 'bottom'
        }

        if (/^\d+$/.test(y) && Number(y) <= 150) {
            return 'top'
        }

        return 'custom'
    },
    set(choice) {
        if (choice !== 'custom') {
            form.value.position_y = POSITION_VALUES[choice]
        }
    },
})

const sizeChoice = computed<SizeChoice>({
    get() {
        const size = Number(form.value.font_size)

        if (size <= 20) {
            return 'small'
        }

        return size <= 30 ? 'medium' : 'large'
    },
    set(choice) {
        form.value.font_size = SIZE_VALUES[choice]
    },
})

const isScrolling = computed({
    get: () => form.value.scroll_direction !== 'none',
    set(value: boolean) {
        if (!value) {
            form.value.scroll_direction = 'none'
        } else if (form.value.scroll_direction === 'none') {
            form.value.scroll_direction = 'right_to_left'
        }
    },
})

onMounted(() => {
    getPreset(-1)
    getFontFamilies()
})

watch([i], () => {
    nextTick(() => {
        getPreset(-1)
    })
})

async function getPreset(index: number) {
    authFetch<TextPreset[]>(`/api/presets/${configStore.channels[configStore.i]?.id}`, {
        method: 'GET',
        headers: authStore.authHeader,
    }).then((data) => {
        if (index === -1) {
            presets.value = [{ value: -1, name: '' }]

            for (let i = 0; i < data.length; i++) {
                const elem = data[i]
                presets.value.push({ value: i, name: elem.name })
            }

            const persistentPreset = data.find((preset) => preset.persistent)

            if (persistentPreset) {
                selected.value = persistentPreset.name
                form.value = persistentPreset
            } else {
                selected.value = null
                form.value = defaultForm()
            }
        } else {
            form.value = data[index]
        }
    })
}

async function getFontFamilies() {
    const response = await authFetch<string[]>('/api/text/fonts', {
        method: 'GET',
        headers: authStore.authHeader,
    })
    fontFamilies.value = response
}

function onChange(event: Event) {
    const select = event.target as HTMLSelectElement
    selected.value = select.value || null

    if (select.selectedIndex === 0) {
        form.value = defaultForm()
        return
    }

    getPreset(select.selectedIndex - 1)
}

async function savePreset() {
    if (selected.value) {
        try {
            await authFetch<TextPreset>(`/api/presets/${configStore.channels[configStore.i]?.id}/${form.value.id}`, {
                method: 'PUT',
                headers: { ...configStore.contentType, ...authStore.authHeader },
                body: JSON.stringify(form.value),
            })

            indexStore.msgAlert('success', t('message.saveDone'), 2)
        } catch {
            indexStore.msgAlert('error', t('message.saveFailed'), 2)
        }
    }
}

function openCreateModal() {
    newPresetName.value = ''
    newPresetError.value = ''
    showCreateModal.value = true

    nextTick(() => newPresetInput.value?.focus())
}

async function createNewPreset(create: boolean) {
    if (create && !newPresetName.value.trim()) {
        newPresetError.value = t('message2.modelNameRequired')
        return
    }

    showCreateModal.value = false

    if (create) {
        const preset = {
            ...form.value,
            name: newPresetName.value.trim(),
            channel_id: configStore.channels[configStore.i]?.id,
        }

        try {
            await authFetch<TextPreset>(`/api/presets/${configStore.channels[configStore.i]?.id}/`, {
                method: 'POST',
                headers: { ...configStore.contentType, ...authStore.authHeader },
                body: JSON.stringify(preset),
            })

            indexStore.msgAlert('success', t('message.saveDone'), 2)
            getPreset(-1)
        } catch {
            indexStore.msgAlert('error', t('message.saveFailed'), 2)
        }
    }

    newPresetName.value = ''
}

async function deletePreset(del: boolean) {
    showDeleteModal.value = false

    if (del && selected.value && selected.value !== '') {
        await authFetch(`/api/presets/${configStore.channels[configStore.i]?.id}/${form.value.id}`, {
            method: 'DELETE',
            headers: authStore.authHeader,
        })

        getPreset(-1)
    }
}

async function postText(payload: TextPreset) {
    await authFetch<void>(`/api/control/${configStore.channels[configStore.i]?.id}/text`, {
        method: 'POST',
        headers: { ...configStore.contentType, ...authStore.authHeader },
        body: JSON.stringify(payload),
    })
}

async function submitMessage() {
    if (!hasContent.value || sending.value) {
        return
    }

    sending.value = true

    try {
        await postText(form.value)

        onScreenText.value =
            form.value.use_filename && !form.value.text.trim() ? t('message2.fileNameShown') : form.value.text
        indexStore.msgAlert('success', t('message2.shownToast'), 3)
    } catch {
        indexStore.msgAlert('error', t('message.sendFailed'), 2)
    }

    sending.value = false
}

// The server removes the overlay when it receives an empty text, so "take off the screen"
// uses the same endpoint and body format with the text emptied.
async function clearMessage() {
    sending.value = true

    try {
        await postText({ ...form.value, text: '', use_filename: false, persistent: false })

        form.value.persistent = false
        onScreenText.value = null
        indexStore.msgAlert('success', t('message2.cleared'), 2)
    } catch {
        indexStore.msgAlert('error', t('message.sendFailed'), 2)
    }

    sending.value = false
}

/*
    Preview helpers
*/
function hexToRgba(hex: string, alpha: number): string {
    const clean = String(hex).replace('#', '')
    const value = clean.length === 6 ? parseInt(clean, 16) : 0
    const safeAlpha = Number.isFinite(alpha) ? Math.min(1, Math.max(0, alpha)) : 1

    return `rgba(${(value >> 16) & 255}, ${(value >> 8) & 255}, ${value & 255}, ${safeAlpha})`
}

function unit(pixels: number): string {
    return `calc(${pixels} * 100cqw / ${REFERENCE_WIDTH})`
}

function axisStyle(value: string, startProp: 'left' | 'top', endProp: 'right' | 'bottom'): Record<string, string> {
    const text = String(value).trim()

    if (text === 'center') {
        return { [startProp]: '50%', transform: startProp === 'left' ? 'translateX(-50%)' : 'translateY(-50%)' }
    }

    if (text.startsWith('end:')) {
        return { [endProp]: unit(Number(text.slice(4)) || 0) }
    }

    return { [startProp]: unit(Number(text) || 0) }
}

const previewText = computed(() => {
    if (form.value.text.trim()) {
        return form.value.text
    }

    return form.value.use_filename ? t('message2.fileNameSample') : t('message2.previewSample')
})

const previewBoxStyle = computed(() => {
    const scrolling = form.value.scroll_direction !== 'none'
    const horizontal: Record<string, string> = scrolling
        ? { left: '0', right: '0' }
        : axisStyle(form.value.position_x, 'left', 'right')
    const vertical = axisStyle(form.value.position_y, 'top', 'bottom')
    const transforms = [horizontal.transform, vertical.transform].filter(Boolean).join(' ')

    return {
        ...horizontal,
        ...vertical,
        transform: transforms || undefined,
        opacity: Number(form.value.opacity),
        maxWidth: scrolling ? undefined : '96%',
        overflow: scrolling ? 'hidden' : undefined,
    }
})

const previewTextStyle = computed(() => {
    const weights = { normal: 400, semibold: 600, bold: 700 }
    const hasBackground = form.value.background_enabled

    return {
        fontFamily: `'${form.value.font_family}', 'DejaVu Sans', sans-serif`,
        fontWeight: weights[form.value.font_weight] ?? 400,
        fontSize: unit(Number(form.value.font_size) || 24),
        lineHeight: 1.15 + (Number(form.value.line_spacing) || 0) / 100,
        color: hexToRgba(form.value.text_color, Number(form.value.text_opacity)),
        backgroundColor: hasBackground
            ? hexToRgba(form.value.background_color, Number(form.value.background_opacity))
            : 'transparent',
        padding: hasBackground ? unit((Number(form.value.background_padding) || 0) + 4) : undefined,
    }
})

const previewAnimationClass = computed(() => {
    if (form.value.scroll_direction === 'right_to_left') {
        return 'ffp-scroll-rtl'
    }

    return form.value.scroll_direction === 'left_to_right' ? 'ffp-scroll-ltr' : ''
})
</script>
<template>
    <div class="w-full px-3 sm:px-6 py-5 max-w-6xl mx-auto">
        <PageHeader :title="t('nav.message')" :subtitle="t('message2.subtitle')" />

        <form @submit.prevent="submitMessage">
            <div class="grid grid-cols-1 gap-5 lg:grid-cols-2 items-start">
                <!-- Left: what to write and how it looks -->
                <div class="space-y-5">
                    <section class="card bg-base-100 shadow p-5">
                        <div class="flex items-center gap-1">
                            <label class="font-bold" for="model-select">{{ t('message2.modelLabel') }}</label>
                            <HelpTip :text="t('message2.modelHelp')" />
                        </div>
                        <div class="mt-2 flex flex-wrap items-center gap-2">
                            <select
                                id="model-select"
                                :value="selected ?? ''"
                                class="select w-full sm:w-72"
                                @change="onChange($event)"
                            >
                                <option v-for="item in presets" :key="item.name" :value="item.name">
                                    {{ item.name || t('message2.blankModel') }}
                                </option>
                            </select>
                        </div>
                        <div class="mt-3 flex flex-wrap gap-2">
                            <button type="button" class="btn btn-sm" :disabled="!selected" @click="savePreset()">
                                <i class="bi bi-cloud-upload" />
                                {{ t('message2.saveModel') }}
                            </button>
                            <button type="button" class="btn btn-sm" @click="openCreateModal">
                                <i class="bi bi-file-plus" />
                                {{ t('message2.saveAsNew') }}
                            </button>
                            <button
                                type="button"
                                class="btn btn-sm btn-ghost text-error"
                                :disabled="!selected"
                                @click="showDeleteModal = true"
                            >
                                <i class="bi bi-trash3" />
                                {{ t('message2.deleteModel') }}
                            </button>
                        </div>
                    </section>

                    <section class="card bg-base-100 shadow p-5 space-y-5">
                        <div>
                            <label class="font-bold text-lg" for="message-text">{{ t('message2.textLabel') }}</label>
                            <textarea
                                id="message-text"
                                v-model="form.text"
                                class="textarea w-full mt-2 text-lg"
                                rows="3"
                                :placeholder="t('message2.textPlaceholder')"
                            />
                        </div>

                        <fieldset>
                            <legend class="font-bold mb-2">{{ t('message2.positionLabel') }}</legend>
                            <div class="join w-full">
                                <button
                                    v-for="choice in ['top', 'middle', 'bottom'] as const"
                                    :key="choice"
                                    type="button"
                                    class="btn join-item grow"
                                    :class="positionChoice === choice && 'btn-primary'"
                                    :aria-pressed="positionChoice === choice"
                                    @click="positionChoice = choice"
                                >
                                    <i
                                        class="bi"
                                        :class="{
                                            'bi-align-top': choice === 'top',
                                            'bi-align-middle': choice === 'middle',
                                            'bi-align-bottom': choice === 'bottom',
                                        }"
                                    />
                                    {{ t(`message2.position.${choice}`) }}
                                </button>
                            </div>
                            <p v-if="positionChoice === 'custom'" class="mt-1 text-sm opacity-70">
                                {{ t('message2.positionCustom') }}
                            </p>
                        </fieldset>

                        <fieldset>
                            <legend class="font-bold mb-2">{{ t('message2.sizeLabel') }}</legend>
                            <div class="join w-full">
                                <button
                                    v-for="choice in ['small', 'medium', 'large'] as const"
                                    :key="choice"
                                    type="button"
                                    class="btn join-item grow"
                                    :class="sizeChoice === choice && 'btn-primary'"
                                    :aria-pressed="sizeChoice === choice"
                                    @click="sizeChoice = choice"
                                >
                                    <span
                                        :class="{
                                            'text-sm': choice === 'small',
                                            'text-base': choice === 'medium',
                                            'text-xl font-bold': choice === 'large',
                                        }"
                                    >
                                        Aa
                                    </span>
                                    {{ t(`message2.size.${choice}`) }}
                                </button>
                            </div>
                        </fieldset>

                        <fieldset>
                            <legend class="font-bold mb-2">{{ t('message2.colorLabel') }}</legend>
                            <div class="flex flex-wrap items-center gap-3">
                                <button
                                    v-for="color in COLOR_CHOICES"
                                    :key="color.value"
                                    type="button"
                                    class="flex h-10 w-10 items-center justify-center rounded-full border-2 border-base-content/30 cursor-pointer"
                                    :class="
                                        form.text_color.toLowerCase() === color.value &&
                                        'ring-3 ring-primary ring-offset-2 ring-offset-base-100'
                                    "
                                    :style="{ backgroundColor: color.value }"
                                    :title="t(`message2.color.${color.key}`)"
                                    :aria-label="t(`message2.color.${color.key}`)"
                                    :aria-pressed="form.text_color.toLowerCase() === color.value"
                                    @click="form.text_color = color.value"
                                >
                                    <i
                                        v-if="form.text_color.toLowerCase() === color.value"
                                        class="bi bi-check-lg text-lg"
                                        :class="
                                            color.value === '#ffffff' || color.value === '#ffd400'
                                                ? 'text-black'
                                                : 'text-white'
                                        "
                                    />
                                </button>
                                <label class="flex items-center gap-2 text-sm cursor-pointer">
                                    <input
                                        v-model="form.text_color"
                                        type="color"
                                        class="h-10 w-12 cursor-pointer rounded-lg border border-base-content/30 bg-transparent p-0.5"
                                        :aria-label="t('message2.color.other')"
                                    />
                                    {{ t('message2.color.other') }}
                                </label>
                            </div>
                        </fieldset>

                        <div class="grid sm:grid-cols-2 gap-5">
                            <label class="flex items-start gap-3 cursor-pointer">
                                <input
                                    v-model="form.background_enabled"
                                    type="checkbox"
                                    class="toggle toggle-primary mt-0.5"
                                />
                                <span>
                                    <span class="block font-bold">{{ t('message2.backgroundLabel') }}</span>
                                    <span class="block text-sm opacity-70">{{ t('message2.backgroundHelp') }}</span>
                                </span>
                            </label>

                            <fieldset>
                                <legend class="font-bold mb-1">{{ t('message2.motionLabel') }}</legend>
                                <div class="join w-full">
                                    <button
                                        type="button"
                                        class="btn btn-sm join-item grow"
                                        :class="!isScrolling && 'btn-primary'"
                                        :aria-pressed="!isScrolling"
                                        @click="isScrolling = false"
                                    >
                                        <i class="bi bi-pause-circle" />
                                        {{ t('message2.motionStill') }}
                                    </button>
                                    <button
                                        type="button"
                                        class="btn btn-sm join-item grow"
                                        :class="isScrolling && 'btn-primary'"
                                        :aria-pressed="isScrolling"
                                        @click="isScrolling = true"
                                    >
                                        <i class="bi bi-arrow-left-circle" />
                                        {{ t('message2.motionScroll') }}
                                    </button>
                                </div>
                            </fieldset>
                        </div>
                    </section>
                </div>

                <!-- Right: preview and the two big buttons -->
                <div class="space-y-5 lg:sticky lg:top-4">
                    <section class="card bg-base-100 shadow p-5">
                        <div class="flex items-center gap-1 font-bold">
                            {{ t('message2.previewTitle') }}
                            <HelpTip :text="t('message2.previewHelp')" />
                        </div>
                        <div
                            class="relative mt-3 aspect-video overflow-hidden rounded-xl bg-linear-to-br from-slate-600 to-slate-900 [container-type:inline-size]"
                            role="img"
                            :aria-label="t('message2.previewTitle')"
                        >
                            <div class="absolute inset-0 flex items-center justify-center gap-2 text-white/25 text-sm">
                                <i class="bi bi-tv text-2xl" />
                                {{ t('message2.previewScreen') }}
                            </div>
                            <div class="absolute" :style="previewBoxStyle">
                                <div
                                    data-audit-ignore
                                    class="whitespace-pre-line wrap-break-word w-fit"
                                    :class="[previewAnimationClass, previewAnimationClass && 'whitespace-nowrap']"
                                    :style="previewTextStyle"
                                >
                                    {{ previewText }}
                                </div>
                            </div>
                        </div>
                        <p class="mt-2 text-xs opacity-60">{{ t('message2.previewNote') }}</p>
                    </section>

                    <section class="card bg-base-100 shadow p-5">
                        <p
                            v-if="onScreenText !== null"
                            class="mb-3 flex items-start gap-2 rounded-lg bg-success/15 p-3 text-success font-semibold"
                            role="status"
                        >
                            <i class="bi bi-broadcast mt-0.5" />
                            <span class="min-w-0 wrap-break-word">
                                {{ t('message2.onScreenNow') }}
                                <span class="font-normal text-base-content">{{ onScreenText }}</span>
                            </span>
                        </p>
                        <button
                            type="submit"
                            class="btn btn-primary btn-lg w-full text-xl"
                            :disabled="!hasContent || sending"
                        >
                            <span v-if="sending" class="loading loading-spinner" />
                            <i v-else class="bi bi-display" />
                            {{ t('message2.show') }}
                        </button>
                        <button type="button" class="btn btn-lg w-full mt-3" :disabled="sending" @click="clearMessage">
                            <i class="bi bi-eye-slash" />
                            {{ t('message2.hide') }}
                        </button>
                        <p v-if="!hasContent" class="mt-3 text-sm opacity-70">{{ t('message2.needText') }}</p>
                    </section>
                </div>

                <!-- Advanced: every original technical field -->
                <section class="card bg-base-100 shadow p-5 lg:col-span-2">
                    <button
                        type="button"
                        class="flex min-h-10 w-full items-center gap-2 text-left font-bold cursor-pointer"
                        :aria-expanded="showAdvanced"
                        @click="showAdvanced = !showAdvanced"
                    >
                        <i class="bi" :class="showAdvanced ? 'bi-chevron-down' : 'bi-chevron-right'" />
                        {{ showAdvanced ? t('ui.advancedHide') : t('ui.advancedShow') }}
                    </button>
                    <p class="mt-1 text-sm opacity-70">{{ t('ui.advancedHelp') }}</p>

                    <div v-if="showAdvanced" class="mt-4">
                        <div class="grid grid-cols-1 md:grid-cols-[minmax(0,1fr)_160px_auto_auto] gap-4">
                            <fieldset class="fieldset min-w-0">
                                <legend class="fieldset min-w-0-legend">{{ t('message2.adv.font') }}</legend>
                                <select v-model="form.font_family" class="select w-full">
                                    <option
                                        v-if="form.font_family && !fontFamilies.includes(form.font_family)"
                                        :value="form.font_family"
                                    >
                                        {{ form.font_family }}
                                    </option>
                                    <option v-for="family in fontFamilies" :key="family" :value="family">
                                        {{ family }}
                                    </option>
                                </select>
                            </fieldset>
                            <fieldset class="fieldset min-w-0">
                                <legend class="fieldset min-w-0-legend">{{ t('message2.adv.weight') }}</legend>
                                <select v-model="form.font_weight" class="select w-full">
                                    <option value="normal">{{ t('message2.adv.weightNormal') }}</option>
                                    <option value="semibold">{{ t('message2.adv.weightSemibold') }}</option>
                                    <option value="bold">{{ t('message2.adv.weightBold') }}</option>
                                </select>
                            </fieldset>
                            <fieldset class="fieldset min-w-0 rounded-box">
                                <label class="fieldset min-w-0-label text-base-content">
                                    <input v-model="form.use_filename" type="checkbox" class="checkbox" />
                                    {{ t('message2.adv.useFilename') }}
                                </label>
                            </fieldset>
                            <fieldset class="fieldset min-w-0 rounded-box">
                                <label class="fieldset min-w-0-label text-base-content">
                                    <input
                                        v-model="form.persistent"
                                        type="checkbox"
                                        class="checkbox"
                                        :disabled="form.id <= 0"
                                    />
                                    {{ t('message2.adv.persistent') }}
                                    <HelpTip :text="t('message2.adv.persistentHelp')" />
                                </label>
                            </fieldset>
                        </div>
                        <fieldset v-if="form.use_filename" class="fieldset min-w-0">
                            <legend class="fieldset min-w-0-legend">{{ t('message2.adv.filenameRegex') }}</legend>
                            <input v-model="form.filename_regex" type="text" class="input w-full" />
                        </fieldset>

                        <div class="mt-2 grid grid-cols-1 xs:grid-cols-[auto_150px_150px] gap-4">
                            <div class="grow">
                                <fieldset class="fieldset min-w-0">
                                    <legend class="fieldset min-w-0-legend">{{ t('message.xAxis') }}</legend>
                                    <input v-model="form.position_x" type="text" name="x" class="input w-full" />
                                    <p class="label whitespace-normal">{{ t('message2.adv.axisHelp') }}</p>
                                </fieldset>
                                <fieldset class="fieldset min-w-0">
                                    <legend class="fieldset min-w-0-legend">{{ t('message.yAxis') }}</legend>
                                    <input v-model="form.position_y" type="text" name="y" class="input w-full" />
                                </fieldset>
                            </div>

                            <div class="xs:mt-8.5">
                                <fieldset class="fieldset min-w-0 rounded-box w-full">
                                    <label class="fieldset min-w-0-label text-base-content">
                                        <input v-model="form.background_enabled" type="checkbox" class="checkbox" />
                                        {{ t('message.showBox') }}
                                    </label>
                                </fieldset>
                                <fieldset class="fieldset min-w-0 mt-1">
                                    <legend class="fieldset min-w-0-legend">{{ t('message.boxColor') }}</legend>
                                    <input
                                        v-model="form.background_color"
                                        type="color"
                                        class="input w-full cursor-pointer"
                                    />
                                </fieldset>
                            </div>
                            <fieldset class="fieldset min-w-0 mt-1 xs:mt-17.5">
                                <legend class="fieldset min-w-0-legend">{{ t('message.boxAlpha') }}</legend>
                                <input
                                    v-model="form.background_opacity"
                                    type="number"
                                    min="0"
                                    max="1"
                                    step="0.01"
                                    class="input w-full"
                                    required
                                />
                            </fieldset>
                        </div>
                        <div class="grid grid-cols-1 xs:grid-cols-[150px_150px_auto] gap-4 mt-2">
                            <div>
                                <fieldset class="fieldset min-w-0">
                                    <legend class="fieldset min-w-0-legend">{{ t('message.size') }}</legend>
                                    <input
                                        v-model="form.font_size"
                                        type="number"
                                        min="0"
                                        class="input w-full"
                                        required
                                    />
                                </fieldset>
                                <fieldset class="fieldset min-w-0">
                                    <legend class="fieldset min-w-0-legend">{{ t('message.fontColor') }}</legend>
                                    <input v-model="form.text_color" type="color" class="input w-full cursor-pointer" />
                                </fieldset>
                            </div>
                            <div>
                                <fieldset class="fieldset min-w-0">
                                    <legend class="fieldset min-w-0-legend">{{ t('message.spacing') }}</legend>
                                    <input
                                        v-model="form.line_spacing"
                                        type="number"
                                        min="0"
                                        class="input w-full"
                                        required
                                    />
                                </fieldset>
                                <fieldset class="fieldset min-w-0">
                                    <legend class="fieldset min-w-0-legend">{{ t('message.fontAlpha') }}</legend>
                                    <input
                                        v-model="form.text_opacity"
                                        type="number"
                                        min="0"
                                        max="1"
                                        step="0.01"
                                        class="input w-full"
                                        required
                                    />
                                </fieldset>
                            </div>

                            <div class="grow">
                                <fieldset class="fieldset min-w-0">
                                    <legend class="fieldset min-w-0-legend">{{ t('message.overallAlpha') }}</legend>
                                    <input
                                        v-model="form.opacity"
                                        type="number"
                                        min="0"
                                        max="1"
                                        step="0.01"
                                        class="input w-full"
                                        required
                                    />
                                </fieldset>
                                <fieldset class="fieldset min-w-0">
                                    <legend class="fieldset min-w-0-legend">{{ t('message.borderWidth') }}</legend>
                                    <input
                                        v-model="form.background_padding"
                                        type="number"
                                        min="0"
                                        class="input w-full"
                                        required
                                    />
                                </fieldset>
                            </div>
                        </div>

                        <div class="grid grid-cols-1 xs:grid-cols-4 gap-4 mt-2">
                            <fieldset class="fieldset min-w-0">
                                <legend class="fieldset min-w-0-legend">{{ t('message2.adv.scrolling') }}</legend>
                                <select v-model="form.scroll_direction" class="select w-full">
                                    <option value="none">{{ t('message2.adv.scrollNone') }}</option>
                                    <option value="left_to_right">{{ t('message2.adv.scrollLtr') }}</option>
                                    <option value="right_to_left">{{ t('message2.adv.scrollRtl') }}</option>
                                </select>
                            </fieldset>
                            <fieldset class="fieldset min-w-0">
                                <legend class="fieldset min-w-0-legend">{{ t('message2.adv.speed') }}</legend>
                                <input v-model="form.scroll_speed" type="number" min="1" class="input w-full" />
                            </fieldset>
                            <fieldset class="fieldset min-w-0">
                                <legend class="fieldset min-w-0-legend">{{ t('message2.adv.repeat') }}</legend>
                                <input v-model="form.scroll_repeat" type="number" min="-1" class="input w-full" />
                                <p class="label whitespace-normal">{{ t('message2.adv.repeatHelp') }}</p>
                            </fieldset>
                            <div class="grid grid-cols-2 gap-2">
                                <fieldset class="fieldset min-w-0">
                                    <legend class="fieldset min-w-0-legend">{{ t('message2.adv.fadeIn') }}</legend>
                                    <input
                                        v-model="form.fade_in_seconds"
                                        type="number"
                                        min="0"
                                        step="0.1"
                                        class="input w-full"
                                    />
                                </fieldset>
                                <fieldset class="fieldset min-w-0">
                                    <legend class="fieldset min-w-0-legend">{{ t('message2.adv.fadeOut') }}</legend>
                                    <input
                                        v-model="form.fade_out_seconds"
                                        type="number"
                                        min="0"
                                        step="0.1"
                                        class="input w-full"
                                    />
                                </fieldset>
                            </div>
                        </div>
                    </div>
                </section>
            </div>
        </form>

        <ConfirmModal
            :show="showCreateModal"
            :title="t('message2.saveAsNew')"
            :confirm-label="t('message2.createModelYes')"
            @confirm="createNewPreset(true)"
            @cancel="createNewPreset(false)"
        >
            <label class="mt-3 block">
                <span class="text-sm font-semibold">{{ t('message2.modelNameLabel') }}</span>
                <input
                    ref="newPresetInput"
                    v-model="newPresetName"
                    type="text"
                    class="input w-full mt-1"
                    :placeholder="t('message2.modelNamePlaceholder')"
                    @keydown.enter.prevent="createNewPreset(true)"
                />
            </label>
            <p v-if="newPresetError" class="mt-2 text-sm font-semibold text-error">{{ newPresetError }}</p>
        </ConfirmModal>

        <ConfirmModal
            :show="showDeleteModal"
            danger
            :title="t('message2.deleteModelTitle')"
            :confirm-label="t('message2.deleteModelYes')"
            @confirm="deletePreset(true)"
            @cancel="deletePreset(false)"
        >
            <p class="mt-2 rounded-lg bg-base-200 p-2 font-semibold break-words">{{ selected }}</p>
            <p class="mt-3 opacity-80">{{ t('message2.deleteModelText') }}</p>
        </ConfirmModal>
    </div>
</template>
<style>
@keyframes ffp-scroll-rtl {
    from {
        transform: translateX(100cqw);
    }

    to {
        transform: translateX(-100%);
    }
}

@keyframes ffp-scroll-ltr {
    from {
        transform: translateX(-100%);
    }

    to {
        transform: translateX(100cqw);
    }
}

.ffp-scroll-rtl {
    animation: ffp-scroll-rtl 9s linear infinite;
}

.ffp-scroll-ltr {
    animation: ffp-scroll-ltr 9s linear infinite;
}

@media (prefers-reduced-motion: reduce) {
    .ffp-scroll-rtl,
    .ffp-scroll-ltr {
        animation: none;
    }
}
</style>
