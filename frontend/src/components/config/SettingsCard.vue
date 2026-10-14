<script setup lang="ts">
defineProps<{
    title: string
    text?: string
    icon?: string
    danger?: boolean
}>()
</script>
<template>
    <section class="card bg-base-100 shadow" :class="danger && 'border border-error/40'">
        <div class="card-body gap-4 p-4 sm:p-6">
            <header class="flex items-start gap-3">
                <i v-if="icon" class="bi text-2xl shrink-0" :class="[icon, danger ? 'text-error' : 'text-primary']" />
                <div class="min-w-0 grow">
                    <h2 class="text-lg font-bold leading-tight">{{ title }}</h2>
                    <p v-if="text" class="mt-1 text-sm opacity-70">{{ text }}</p>
                </div>
                <slot name="actions" />
            </header>
            <slot />
        </div>
    </section>
</template>
