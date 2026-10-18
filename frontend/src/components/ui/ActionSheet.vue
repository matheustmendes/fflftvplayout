<script setup lang="ts">
import { onBeforeUnmount, watch } from 'vue'
import { useI18n } from 'vue-i18n'

const props = withDefaults(defineProps<{ show: boolean; title?: string }>(), { title: '' })
const emit = defineEmits<{ (event: 'close'): void }>()

const { t } = useI18n()

function onKey(event: KeyboardEvent) {
    if (event.key === 'Escape') {
        emit('close')
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
    <Teleport to="body">
        <div
            v-if="show"
            class="fixed inset-0 z-50 flex items-end justify-center bg-black/40 md:items-center md:p-4"
            role="dialog"
            aria-modal="true"
            :aria-label="title || undefined"
            @click.self="emit('close')"
        >
            <!-- Folha que sobe de baixo no celular (alcance do polegar); janela no computador. -->
            <div class="pb-safe w-full max-w-md rounded-t-2xl bg-base-100 shadow-2xl md:rounded-2xl">
                <div class="mx-auto mt-2 h-1 w-10 rounded-full bg-base-300 md:hidden" aria-hidden="true" />
                <p v-if="title" class="truncate px-5 pt-3 text-sm font-semibold opacity-70">{{ title }}</p>
                <div class="flex flex-col gap-1 p-3" @click="emit('close')">
                    <slot />
                </div>
                <div class="border-t border-base-300 p-3">
                    <button type="button" class="btn w-full" @click="emit('close')">{{ t('ui.close') }}</button>
                </div>
            </div>
        </div>
    </Teleport>
</template>
