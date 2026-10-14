<script setup lang="ts">
import { useI18n } from 'vue-i18n'

import { WEEKDAYS } from '@/composables/commercialApi'

defineProps<{ modelValue: number }>()
const emit = defineEmits<{ (event: 'update:modelValue', value: number): void }>()

const { t } = useI18n()
</script>
<template>
    <div role="tablist" class="tabs tabs-box tabs-sm flex-wrap">
        <button
            v-for="day in WEEKDAYS"
            :key="day"
            role="tab"
            class="tab"
            :class="modelValue === day && 'tab-active'"
            @click="emit('update:modelValue', day)"
        >
            {{ t(`commercial.weekday.${day}`) }}
        </button>
    </div>
</template>
