<script setup lang="ts">
import { computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useHead } from '@unhead/vue'
import { RouterView, useRoute, useRouter } from 'vue-router'

import PageHeader from '@/components/ui/PageHeader.vue'
import ScrollTabs, { type ScrollTab } from '@/components/ui/ScrollTabs.vue'

import { usePlayoutDirty } from '@/composables/settingsHelpers'
import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'

interface SettingsTab {
    name: string
    label: string
    icon: string
    // Tabs that edit the playout configuration share one "unsaved" state.
    playout?: boolean
}

const { t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const route = useRoute()
const router = useRouter()
const playoutDirty = usePlayoutDirty()

useHead({
    title: computed(() => t('settings2.title')),
})

const isOperator = computed(() => authStore.role === 'user')
const channelQuery = computed(() => ({ channel: route.query.channel }))
const channelName = computed(() => configStore.channels[configStore.i]?.name ?? '')

const tabs = computed<SettingsTab[]>(() => {
    // The operator only sees the own account.
    if (isOperator.value) {
        return [{ name: 'configure-user', label: t('settings2.tabs.account'), icon: 'bi-person-circle' }]
    }

    const all: SettingsTab[] = [
        { name: 'configure-channel', label: t('settings2.tabs.channel'), icon: 'bi-tv' },
        { name: 'configure-playout', label: t('settings2.tabs.output'), icon: 'bi-broadcast', playout: true },
        { name: 'configure-audio', label: t('settings2.tabs.audio'), icon: 'bi-volume-up', playout: true },
        { name: 'configure-recording', label: t('settings2.tabs.recording'), icon: 'bi-record-circle', playout: true },
        { name: 'configure-telegram', label: t('settings2.tabs.telegram'), icon: 'bi-telegram' },
        {
            name: 'configure-user',
            label: authStore.role === 'global_admin' ? t('settings2.tabs.accounts') : t('settings2.tabs.account'),
            icon: authStore.role === 'global_admin' ? 'bi-people' : 'bi-person-circle',
        },
    ]

    if (authStore.role === 'global_admin') {
        all.push({ name: 'configure-global', label: t('settings2.tabs.system'), icon: 'bi-gear-wide-connected' })
    }

    return all
})

const tabList = computed<ScrollTab[]>(() =>
    tabs.value.map((tab) => ({
        name: tab.name,
        label: tab.label,
        icon: tab.icon,
        dirty: Boolean(tab.playout && playoutDirty.value),
    })),
)

// The router lands everyone on the channel tab; the operator has no such tab.
watch(
    [isOperator, () => route.name],
    () => {
        if (isOperator.value && route.name === 'configure-channel') {
            void router.replace({ name: 'configure-user', query: route.query })
        }
    },
    { immediate: true },
)
</script>
<template>
    <div class="w-full max-w-5xl mx-auto px-4 sm:px-6 pt-5 pb-10">
        <PageHeader :title="t('settings2.title')" :subtitle="t('settings2.subtitle')">
            <template v-if="configStore.channels.length > 1 && channelName" #actions>
                <span class="badge badge-lg badge-outline gap-2">
                    <i class="bi bi-tv" />
                    {{ t('settings2.channelChip', { name: channelName }) }}
                </span>
            </template>
        </PageHeader>

        <ScrollTabs
            v-if="tabs.length > 1"
            class="mb-5"
            :tabs="tabList"
            :query="channelQuery"
            :dirty-label="t('settings2.unsavedDot')"
        />

        <RouterView />
    </div>
</template>
