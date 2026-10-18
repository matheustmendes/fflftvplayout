<script setup lang="ts">
import logo from '@/assets/images/lftv.png'

withDefaults(
    defineProps<{
        title: string
        subtitle?: string
        wide?: boolean
    }>(),
    { subtitle: '', wide: false },
)
</script>
<template>
    <main class="min-h-screen min-h-dvh w-full flex items-center justify-center px-3 sm:px-4 py-6 sm:py-10 bg-base-200">
        <div class="w-full" :class="wide ? 'max-w-3xl' : 'max-w-md'">
            <div class="flex flex-col items-center text-center mb-5">
                <img :src="logo" alt="" width="72" height="72" class="h-16 w-16 sm:h-[72px] sm:w-[72px] drop-shadow" />
                <div class="mt-2 text-sm font-bold tracking-[0.25em] uppercase opacity-60">LFTV</div>
                <h1 class="mt-3 text-3xl sm:text-4xl font-bold leading-tight">{{ title }}</h1>
                <p v-if="subtitle" class="mt-2 text-base sm:text-lg opacity-70 max-w-xl">{{ subtitle }}</p>
            </div>
            <div class="rounded-2xl bg-base-100 p-5 sm:p-8 shadow-xl">
                <slot />
            </div>
            <div v-if="$slots.footer" class="mt-4 text-center text-sm opacity-80">
                <slot name="footer" />
            </div>
        </div>
    </main>
</template>
