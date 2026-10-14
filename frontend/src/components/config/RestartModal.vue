<script setup lang="ts">
import { useI18n } from 'vue-i18n'

import ConfirmModal from '@/components/ui/ConfirmModal.vue'

import { useConfig } from '@/stores/config'

const { t } = useI18n()
const configStore = useConfig()
</script>
<template>
    <ConfirmModal
        :show="configStore.showRestartModal"
        danger
        :title="t('settings2.restart.title')"
        :text="t('settings2.restart.text')"
        :confirm-label="t('settings2.restart.confirm')"
        :cancel-label="t('settings2.restart.later')"
        @confirm="configStore.restart(true)"
        @cancel="configStore.restart(false)"
    />
</template>
