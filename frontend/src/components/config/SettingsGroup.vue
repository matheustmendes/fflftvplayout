<script setup lang="ts">
defineProps<{
    title: string
    text?: string
}>()
</script>
<template>
    <section class="rounded-box border border-base-300 bg-base-200/40 p-4 space-y-3">
        <header>
            <h3 class="font-semibold">{{ title }}</h3>
            <p v-if="text" class="mt-0.5 text-sm opacity-70 whitespace-pre-line">{{ text }}</p>
        </header>
        <slot />
    </section>
</template>
