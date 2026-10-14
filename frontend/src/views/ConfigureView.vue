<script setup lang="ts">
import { computed, nextTick, useTemplateRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useHead } from '@unhead/vue'
import { RouterLink, RouterView, useRoute, useRouter } from 'vue-router'

import PageHeader from '@/components/ui/PageHeader.vue'

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
const tabList = useTemplateRef<HTMLElement>('tabList')

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

// On a phone the tab row scrolls sideways: keep the current tab in sight.
watch(
    () => route.name,
    async () => {
        await nextTick()
        tabList.value?.querySelector('[aria-current="page"]')?.scrollIntoView({ inline: 'center', block: 'nearest' })
    },
    { immediate: true },
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

        <nav
            v-if="tabs.length > 1"
            ref="tabList"
            role="tablist"
            class="tabs tabs-border mb-5 flex-nowrap overflow-x-auto border-b border-base-300"
        >
            <RouterLink
                v-for="tab in tabs"
                :key="tab.name"
                role="tab"
                :to="{ name: tab.name, query: channelQuery }"
                class="tab h-12 shrink-0 gap-2 whitespace-nowrap px-4 text-base"
                :class="route.name === tab.name ? 'tab-active font-semibold' : 'text-base-content/75'"
            >
                <i class="bi" :class="tab.icon" />
                {{ tab.label }}
                <span
                    v-if="tab.playout && playoutDirty"
                    class="h-2.5 w-2.5 rounded-full bg-warning"
                    :title="t('settings2.unsavedDot')"
                    role="img"
                    :aria-label="t('settings2.unsavedDot')"
                />
            </RouterLink>
        </nav>

        <RouterView />
    </div>
</template>
