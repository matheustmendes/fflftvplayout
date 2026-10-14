<script setup lang="ts">
import { useI18n } from 'vue-i18n'

import ConfirmModal from '@/components/ui/ConfirmModal.vue'

defineProps<{ show: boolean }>()
const emit = defineEmits<{
    (event: 'confirm'): void
    (event: 'cancel'): void
}>()

const { t } = useI18n()
</script>
<template>
    <ConfirmModal
        :show="show"
        :title="t('settings2.save.discardTitle')"
        :text="t('settings2.save.discardText')"
        :confirm-label="t('settings2.save.discardConfirm')"
        :cancel-label="t('settings2.save.discardKeep')"
        @confirm="emit('confirm')"
        @cancel="emit('cancel')"
    />
</template>
