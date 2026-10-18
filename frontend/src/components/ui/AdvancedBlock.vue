<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'

const props = withDefaults(defineProps<{ startOpen?: boolean }>(), { startOpen: false })

const { t } = useI18n()
const open = ref(props.startOpen)
</script>
<template>
    <section class="card bg-base-100 shadow">
        <button
            type="button"
            class="flex min-h-14 w-full items-center gap-3 p-4 text-start rounded-2xl hover:bg-base-200/60"
            :aria-expanded="open"
            @click="open = !open"
        >
            <i class="bi bi-sliders text-2xl opacity-70 shrink-0" />
            <span class="grow min-w-0">
                <span class="block font-semibold">
                    {{ open ? t('ui.advancedHide') : t('ui.advancedShow') }}
                </span>
                <span class="block text-sm opacity-70">{{ t('ui.advancedHelp') }}</span>
            </span>
            <i class="bi shrink-0" :class="open ? 'bi-chevron-up' : 'bi-chevron-down'" />
        </button>
        <div v-if="open" class="border-t border-base-300 p-4 space-y-4">
            <slot />
        </div>
    </section>
</template>
