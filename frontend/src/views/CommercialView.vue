<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useHead } from '@unhead/vue'
import { RouterLink, RouterView, useRoute } from 'vue-router'

import { useAuth } from '@/stores/auth'

const { t } = useI18n()
const authStore = useAuth()
const route = useRoute()

useHead({
    title: computed(() => t('button.commercial')),
})

const channelQuery = computed(() => ({ channel: route.query.channel }))
const tabs = computed(() => {
    const all = [
        { name: 'commercial-today', label: t('commercial.tab.today') },
        { name: 'commercial-spots', label: t('commercial.tab.spots') },
        { name: 'commercial-campaigns', label: t('commercial.tab.campaigns') },
        { name: 'commercial-breaks', label: t('commercial.tab.breaks') },
        { name: 'commercial-grid', label: t('commercial.tab.grid') },
        { name: 'commercial-reports', label: t('commercial.tab.reports') },
    ]

    return authStore.role === 'user'
        ? all
        : [...all, { name: 'commercial-settings', label: t('commercial.tab.settings') }]
})
</script>
<template>
    <div class="w-full px-2 sm:px-6 pb-10">
        <div role="tablist" class="tabs tabs-border mt-4 overflow-x-auto flex-nowrap">
            <RouterLink
                v-for="tab in tabs"
                :key="tab.name"
                role="tab"
                :to="{ name: tab.name, query: channelQuery }"
                class="tab whitespace-nowrap"
                :class="route.name === tab.name && 'tab-active font-semibold'"
            >
                {{ tab.label }}
            </RouterLink>
        </div>
        <div class="mt-4 max-w-6xl mx-auto">
            <RouterView />
        </div>
    </div>
</template>
