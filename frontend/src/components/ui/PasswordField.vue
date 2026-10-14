<script setup lang="ts">
import { ref, useTemplateRef } from 'vue'
import { useI18n } from 'vue-i18n'

const props = withDefaults(
    defineProps<{
        label: string
        name?: string
        autocomplete?: string
        placeholder?: string
        hint?: string
        invalid?: boolean
        required?: boolean
    }>(),
    { name: 'password', autocomplete: 'current-password', placeholder: '', hint: '', invalid: false, required: true },
)
const model = defineModel<string>({ required: true })

const { t } = useI18n()
const visible = ref(false)
const input = useTemplateRef<HTMLInputElement>('input')
const inputId = `pw-${props.name}-${Math.random().toString(36).slice(2, 8)}`

defineExpose({ focus: () => input.value?.focus() })
</script>
<template>
    <div>
        <label class="block text-sm font-semibold mb-1" :for="inputId">{{ label }}</label>
        <div class="input input-lg w-full pe-1" :class="invalid && 'input-error'">
            <input
                :id="inputId"
                ref="input"
                v-model="model"
                :type="visible ? 'text' : 'password'"
                :name="name"
                :autocomplete="autocomplete"
                :placeholder="placeholder"
                :required="required"
                autocapitalize="off"
                spellcheck="false"
            />
            <button
                type="button"
                class="btn btn-ghost btn-sm shrink-0"
                :aria-pressed="visible"
                :aria-label="visible ? t('auth2.hidePassword') : t('auth2.showPassword')"
                @click="visible = !visible"
            >
                <i class="bi" :class="visible ? 'bi-eye-slash' : 'bi-eye'" />
                {{ visible ? t('auth2.hide') : t('auth2.show') }}
            </button>
        </div>
        <p v-if="hint" class="mt-1 text-sm opacity-70">{{ hint }}</p>
    </div>
</template>
