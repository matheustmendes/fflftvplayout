<script setup lang="ts">
import { onBeforeUnmount, watch } from 'vue'
import { useI18n } from 'vue-i18n'

const props = withDefaults(
    defineProps<{
        show: boolean
        title: string
        text?: string
        confirmLabel?: string
        cancelLabel?: string
        danger?: boolean
    }>(),
    { text: '', confirmLabel: '', cancelLabel: '', danger: false },
)
const emit = defineEmits<{
    (event: 'confirm'): void
    (event: 'cancel'): void
}>()

const { t } = useI18n()

function onKey(event: KeyboardEvent) {
    if (event.key === 'Escape') {
        emit('cancel')
    }
}

watch(
    () => props.show,
    (visible) => {
        document.body.style.overflow = visible ? 'hidden' : ''

        if (visible) {
            window.addEventListener('keydown', onKey)
        } else {
            window.removeEventListener('keydown', onKey)
        }
    },
    { immediate: true },
)

onBeforeUnmount(() => {
    document.body.style.overflow = ''
    window.removeEventListener('keydown', onKey)
})
</script>
<template>
    <div
        v-if="show"
        class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
        role="dialog"
        aria-modal="true"
        @click.self="emit('cancel')"
    >
        <div class="w-full max-w-md rounded-2xl bg-base-100 p-6 shadow-2xl">
            <div class="flex items-start gap-3">
                <i
                    class="bi text-3xl shrink-0"
                    :class="danger ? 'bi-exclamation-triangle-fill text-warning' : 'bi-question-circle-fill text-info'"
                />
                <div class="min-w-0">
                    <h2 class="text-xl font-bold">{{ title }}</h2>
                    <div v-if="text" class="mt-2 opacity-80" v-html="text" />
                    <slot />
                </div>
            </div>
            <div class="mt-6 flex flex-wrap justify-end gap-2">
                <button class="btn" autofocus @click="emit('cancel')">
                    {{ cancelLabel || t('ui.cancel') }}
                </button>
                <button class="btn" :class="danger ? 'btn-error' : 'btn-primary'" @click="emit('confirm')">
                    {{ confirmLabel || t('ui.confirm') }}
                </button>
            </div>
        </div>
    </div>
</template>
