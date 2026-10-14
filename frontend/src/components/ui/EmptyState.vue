<script setup lang="ts">
defineProps<{
    icon?: string
    title: string
    text?: string
}>()
</script>
<template>
    <div class="flex flex-col items-center text-center gap-2 py-8 px-4">
        <i class="bi text-5xl opacity-40" :class="icon ?? 'bi-inbox'" />
        <div class="text-lg font-semibold">{{ title }}</div>
        <p v-if="text" class="max-w-md opacity-70">{{ text }}</p>
        <div class="mt-2 flex flex-wrap justify-center gap-2">
            <slot />
        </div>
    </div>
</template>
