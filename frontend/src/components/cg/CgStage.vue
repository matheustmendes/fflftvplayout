<script setup lang="ts">
import { computed, ref, useTemplateRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { createFilePreviewUrl } from '@/composables/fileAccess'
import { useConfig } from '@/stores/config'
import type { Cg } from '@/composables/cgApi'

const props = defineProps<{ cg: Cg }>()
const emit = defineEmits<{ (event: 'move', x: number, y: number): void }>()

const { t } = useI18n()
const configStore = useConfig()

const stage = useTemplateRef<HTMLElement>('stage')
const item = useTemplateRef<HTMLElement>('item')
const imageUrl = ref('')
const dragging = ref<{ pointer: number; x: number; y: number; startX: number; startY: number } | null>(null)

const round = (value: number) => Math.round(value * 10) / 10
const clamp = (value: number) => Math.min(100, Math.max(0, value))

const itemStyle = computed(() => {
    const base = { left: `${props.cg.x_pct}%`, top: `${props.cg.y_pct}%`, opacity: props.cg.opacity }

    if (props.cg.kind === 'image') {
        return { ...base, width: `${props.cg.size_pct}%` }
    }

    const hex = props.cg.box_color.replace('#', '')
    const rgb = [0, 2, 4].map((start) => parseInt(hex.slice(start, start + 2), 16) || 0)

    return {
        ...base,
        // `cqh` is a percent of the stage height, like the letter height on the air.
        fontSize: `${props.cg.size_pct}cqh`,
        fontWeight: props.cg.font_weight === 'normal' ? 400 : props.cg.font_weight === 'semibold' ? 600 : 700,
        color: props.cg.text_color,
        fontFamily: props.cg.font_family || undefined,
        padding: props.cg.box_enabled ? '0.3em' : '0',
        background: props.cg.box_enabled ? `rgba(${rgb.join(',')},${props.cg.box_opacity})` : 'transparent',
    }
})

watch(
    () => props.cg.image_path,
    async (path) => {
        imageUrl.value = ''

        if (props.cg.kind !== 'image' || !path) {
            return
        }

        try {
            imageUrl.value = await createFilePreviewUrl(configStore.channels[configStore.i]?.id, `/${path}`)
        } catch {
            imageUrl.value = ''
        }
    },
    { immediate: true },
)

function onDown(event: PointerEvent) {
    const target = event.currentTarget as HTMLElement

    target.setPointerCapture(event.pointerId)
    dragging.value = {
        pointer: event.pointerId,
        x: event.clientX,
        y: event.clientY,
        startX: props.cg.x_pct,
        startY: props.cg.y_pct,
    }
}

function onMove(event: PointerEvent) {
    const drag = dragging.value
    const rect = stage.value?.getBoundingClientRect()

    if (!drag || !rect || drag.pointer !== event.pointerId) {
        return
    }

    emit(
        'move',
        round(clamp(drag.startX + ((event.clientX - drag.x) / rect.width) * 100)),
        round(clamp(drag.startY + ((event.clientY - drag.y) / rect.height) * 100)),
    )
}

function onUp() {
    dragging.value = null
}

/** Moves the graphic to a corner, edge or the center, keeping a small margin. */
function place(horizontal: 'left' | 'center' | 'right', vertical: 'top' | 'middle' | 'bottom') {
    const rect = stage.value?.getBoundingClientRect()
    const box = item.value?.getBoundingClientRect()

    if (!rect || !box) {
        return
    }

    const width = (box.width / rect.width) * 100
    const height = (box.height / rect.height) * 100
    const margin = 4
    const x = horizontal === 'left' ? margin : horizontal === 'center' ? (100 - width) / 2 : 100 - width - margin
    const y = vertical === 'top' ? margin : vertical === 'middle' ? (100 - height) / 2 : 100 - height - margin

    emit('move', round(clamp(x)), round(clamp(y)))
}

const horizontals = ['left', 'center', 'right'] as const
const verticals = ['top', 'middle', 'bottom'] as const
</script>
<template>
    <div class="space-y-2">
        <div
            ref="stage"
            class="relative w-full aspect-video overflow-hidden rounded-lg border border-base-300 select-none bg-linear-to-br from-slate-700 to-slate-900"
            style="container-type: size"
            :aria-label="t('cg.editor.stage')"
        >
            <div class="absolute inset-0 opacity-20 pointer-events-none grid grid-cols-3 grid-rows-3">
                <div v-for="cell in 9" :key="cell" class="border border-white/40" />
            </div>
            <div
                ref="item"
                class="absolute leading-tight whitespace-pre cursor-move touch-none outline-dashed outline-1 outline-offset-2 outline-accent"
                :style="itemStyle"
                @pointerdown.prevent="onDown"
                @pointermove="onMove"
                @pointerup="onUp"
                @pointercancel="onUp"
            >
                <template v-if="cg.kind === 'text'">{{ cg.text || t('cg.editor.sample') }}</template>
                <img v-else-if="imageUrl" :src="imageUrl" alt="" class="block w-full h-auto pointer-events-none" />
                <i v-else class="bi bi-image text-white/70 text-4xl" />
            </div>
        </div>

        <div class="flex flex-wrap items-center gap-3">
            <span class="text-sm opacity-70">{{ t('cg.editor.quick') }}</span>
            <div class="grid grid-cols-3 gap-1">
                <template v-for="vertical in verticals" :key="vertical">
                    <button
                        v-for="horizontal in horizontals"
                        :key="`${vertical}-${horizontal}`"
                        type="button"
                        class="btn btn-xs btn-square"
                        :aria-label="`${vertical} ${horizontal}`"
                        @click="place(horizontal, vertical)"
                    >
                        <i class="bi bi-dot" />
                    </button>
                </template>
            </div>
        </div>
    </div>
</template>
