<script setup lang="ts">
import HelpTip from '@/components/ui/HelpTip.vue'

defineProps<{
    label: string
    help?: string
    hint?: string
}>()
</script>
<template>
    <div class="flex flex-col gap-1 min-w-0">
        <div class="flex items-center gap-1 font-semibold">
            <span>{{ label }}</span>
            <HelpTip v-if="help" :text="help" />
        </div>
        <slot />
        <p v-if="hint" class="text-sm opacity-70 whitespace-pre-line">{{ hint }}</p>
    </div>
</template>
