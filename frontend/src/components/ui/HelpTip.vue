<script setup lang="ts">
import { ref, useTemplateRef } from 'vue'
import { onClickOutside } from '@vueuse/core'

defineProps<{ text: string }>()

const open = ref(false)
const root = useTemplateRef<HTMLElement>('root')

onClickOutside(root, () => (open.value = false))
</script>
<template>
    <span ref="root" class="relative inline-block align-middle">
        <button
            type="button"
            class="btn btn-circle btn-ghost btn-xs text-base opacity-70 hover:opacity-100"
            :aria-expanded="open"
            aria-label="?"
            @click="open = !open"
            @keydown.esc="open = false"
        >
            <i class="bi bi-question-circle" />
        </button>
        <span
            v-if="open"
            role="tooltip"
            class="absolute z-30 left-1/2 -translate-x-1/2 top-full mt-1 w-64 max-w-[80vw] rounded-lg bg-base-100 p-3 text-sm font-normal shadow-xl border border-base-300"
        >
            {{ text }}
        </span>
    </span>
</template>
