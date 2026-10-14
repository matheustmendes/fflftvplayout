<script setup lang="ts">
import { useI18n } from 'vue-i18n'

withDefaults(
    defineProps<{
        dirty: boolean
        saving?: boolean
        // When true the save button stays disabled while there is nothing to save.
        requireChange?: boolean
    }>(),
    { saving: false, requireChange: false },
)
const emit = defineEmits<{
    (event: 'discard'): void
}>()

const { t } = useI18n()
</script>
<template>
    <div class="sticky bottom-14 md:bottom-0 z-20 mt-6 pb-2 md:pb-4">
        <div
            class="flex flex-wrap items-center gap-x-4 gap-y-2 rounded-2xl border border-base-300 bg-base-100 p-3 shadow-lg"
            :class="dirty && 'border-warning'"
        >
            <div class="grow min-w-0 flex flex-col">
                <span v-if="dirty" class="flex items-center gap-2 font-semibold text-warning" role="status">
                    <i class="bi bi-exclamation-circle-fill" />
                    {{ t('settings2.save.unsaved') }}
                </span>
                <span v-else class="flex items-center gap-2 opacity-70" role="status">
                    <i class="bi bi-check-circle" />
                    {{ t('settings2.save.allSaved') }}
                </span>
                <span v-if="$slots.default" class="text-sm opacity-70"><slot /></span>
            </div>
            <div class="flex items-center gap-2 ms-auto">
                <button v-if="dirty" type="button" class="btn" :disabled="saving" @click="emit('discard')">
                    {{ t('settings2.save.discard') }}
                </button>
                <button type="submit" class="btn btn-primary" :disabled="saving || (requireChange && !dirty)">
                    <span v-if="saving" class="loading loading-spinner loading-sm" />
                    <i v-else class="bi bi-check-lg" />
                    {{ saving ? t('settings2.save.saving') : t('settings2.save.button') }}
                </button>
            </div>
        </div>
    </div>
</template>
