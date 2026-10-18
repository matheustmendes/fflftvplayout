<script setup lang="ts">
import { computed } from 'vue'

import HelpTip from '@/components/ui/HelpTip.vue'

const props = withDefaults(
    defineProps<{
        label: string
        help?: string
        /** The range of the slider. */
        min: number
        max: number
        step?: number
        /** The most the number box accepts; the slider stops at `max`. */
        inputMax?: number
        unit?: string
    }>(),
    { help: '', step: 1, inputMax: undefined, unit: '%' },
)
const model = defineModel<number>({ required: true })

const upper = computed(() => props.inputMax ?? props.max)
const decimals = computed(() => (props.step < 1 ? Math.min(2, `${props.step}`.split('.')[1]?.length ?? 1) : 0))
// A value past the end of the slider keeps the thumb at the end instead of moving the range.
const thumb = computed(() => Math.min(Math.max(model.value, props.min), props.max))

function round(value: number) {
    const factor = 10 ** decimals.value

    return Math.round(value * factor) / factor
}

function onSlide(event: Event) {
    model.value = round(Number((event.target as HTMLInputElement).value))
}

// The box is free while typing and fixed to the limits only when the person is done.
function onCommit(event: Event) {
    const box = event.target as HTMLInputElement
    const typed = Number(box.value)
    const value =
        Number.isFinite(typed) && box.value !== '' ? Math.min(Math.max(typed, props.min), upper.value) : model.value

    model.value = round(value)
    box.value = `${model.value}`
}
</script>
<template>
    <div class="min-w-0">
        <!-- The header has a fixed height: what is typed here never moves the slider under the finger. -->
        <div class="flex h-9 items-center gap-1">
            <span class="min-w-0 truncate font-semibold">{{ label }}</span>
            <HelpTip v-if="help" :text="help" />
            <span class="ms-auto flex shrink-0 items-center gap-1">
                <input
                    :value="model"
                    type="number"
                    inputmode="decimal"
                    :min="min"
                    :max="upper"
                    :step="step"
                    :aria-label="label"
                    class="input input-xs w-20 text-right tabular-nums"
                    @change="onCommit"
                />
                <span class="w-4 text-sm opacity-70">{{ unit }}</span>
            </span>
        </div>
        <input
            :value="thumb"
            type="range"
            :min="min"
            :max="max"
            :step="step"
            :aria-label="label"
            class="range range-sm range-accent block w-full"
            style="touch-action: pan-y"
            @input="onSlide"
        />
    </div>
</template>
