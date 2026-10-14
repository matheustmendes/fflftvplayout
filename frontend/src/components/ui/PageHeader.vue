<script setup lang="ts">
defineProps<{
    title: string
    subtitle?: string
}>()
</script>
<template>
    <header class="flex flex-wrap items-start gap-3 mb-5">
        <div class="grow min-w-0">
            <h1 class="text-2xl font-bold leading-tight">{{ title }}</h1>
            <p v-if="subtitle" class="mt-1 text-base opacity-70 max-w-3xl">{{ subtitle }}</p>
        </div>
        <div class="flex flex-wrap items-center gap-2">
            <slot name="actions" />
        </div>
    </header>
</template>
