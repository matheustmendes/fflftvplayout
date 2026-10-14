<script setup lang="ts">
import { computed } from 'vue'

export type StatusKind = 'ok' | 'warn' | 'bad' | 'live' | 'idle'

const props = defineProps<{
    kind: StatusKind
    label: string
    size?: 'sm' | 'lg'
}>()

const colors: Record<StatusKind, string> = {
    ok: 'bg-success text-success-content',
    warn: 'bg-warning text-warning-content',
    bad: 'bg-error text-error-content',
    live: 'bg-purple-600 text-white',
    idle: 'bg-base-300 text-base-content',
}

const pulses = computed(() => props.kind === 'ok' || props.kind === 'live' || props.kind === 'warn')
</script>
<template>
    <span
        class="inline-flex items-center gap-2 rounded-full font-bold tracking-wide whitespace-nowrap"
        :class="[colors[kind], size === 'lg' ? 'px-5 py-2 text-xl' : 'px-3 py-1 text-sm']"
        role="status"
    >
        <span class="relative flex h-2.5 w-2.5">
            <span
                v-if="pulses"
                class="absolute inline-flex h-full w-full rounded-full bg-current opacity-60 animate-ping"
            />
            <span class="relative inline-flex h-2.5 w-2.5 rounded-full bg-current" />
        </span>
        {{ label }}
    </span>
</template>
