<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'

const props = withDefaults(
    defineProps<{
        modelValue: Record<string, string>
        namePlaceholder?: string
        valuePlaceholder?: string
        addLabel?: string
        // Metadata, muxer and audio values are trimmed; protocol values may hold secrets, so they stay as typed.
        trimValues?: boolean
    }>(),
    { namePlaceholder: '', valuePlaceholder: '', addLabel: '', trimValues: true },
)
const emit = defineEmits<{
    (event: 'update:modelValue', value: Record<string, string>): void
}>()

const { t } = useI18n()
const newName = ref('')
const newValue = ref('')

const entries = computed(() => Object.entries(props.modelValue))
const cleanName = computed(() => newName.value.trim())
const canAdd = computed(
    () => !!cleanName.value && !!newValue.value.trim() && !Object.hasOwn(props.modelValue, cleanName.value),
)

function eventValue(event: Event): string {
    return (event.target as HTMLInputElement).value
}

function add() {
    if (!canAdd.value) {
        return
    }

    const value = props.trimValues ? newValue.value.trim() : newValue.value
    emit('update:modelValue', { ...props.modelValue, [cleanName.value]: value })
    newName.value = ''
    newValue.value = ''
}

function rename(previousName: string, event: Event) {
    const input = event.target as HTMLInputElement
    const name = input.value.trim()

    if (!name || (name !== previousName && Object.hasOwn(props.modelValue, name))) {
        input.value = previousName
        return
    }

    const options = { ...props.modelValue }
    const value = options[previousName] ?? ''
    delete options[previousName]
    options[name] = value
    emit('update:modelValue', options)
}

function setValue(name: string, value: string) {
    emit('update:modelValue', { ...props.modelValue, [name]: value })
}

function remove(name: string) {
    const options = { ...props.modelValue }
    delete options[name]
    emit('update:modelValue', options)
}
</script>
<template>
    <div class="space-y-2">
        <div v-for="[name, value] in entries" :key="name" class="flex flex-wrap items-center gap-2">
            <input :value="name" type="text" class="input input-sm w-full sm:w-48" @change="rename(name, $event)" />
            <input
                :value="value"
                type="text"
                class="input input-sm grow min-w-0"
                @input="setValue(name, eventValue($event))"
            />
            <button type="button" class="btn btn-sm btn-ghost" @click="remove(name)">
                {{ t('settings2.remove') }}
            </button>
        </div>
        <div class="flex flex-wrap items-center gap-2">
            <input
                v-model="newName"
                type="text"
                class="input input-sm w-full sm:w-48"
                :placeholder="namePlaceholder || t('settings2.kv.name')"
            />
            <input
                v-model="newValue"
                type="text"
                class="input input-sm grow min-w-0"
                :placeholder="valuePlaceholder || t('settings2.kv.value')"
                @keydown.enter.prevent="add"
            />
            <button type="button" class="btn btn-sm" :disabled="!canAdd" @click="add">
                {{ addLabel || t('settings2.kv.add') }}
            </button>
        </div>
    </div>
</template>
