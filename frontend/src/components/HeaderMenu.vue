<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { onClickOutside } from '@vueuse/core'

import StatusPill from '@/components/ui/StatusPill.vue'
import type { StatusKind } from '@/components/ui/StatusPill.vue'

import { locales } from '@/i18n'
import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'
import { useLive } from '@/stores/live'
import type { ChannelState } from '@/stores/live'

interface NavItem {
    key: string
    label: string
    icon: string
    path: string
    /** Roles that see the item; everybody when missing. */
    roles?: string[]
    /** Shown in the bottom bar of the phone. */
    main?: boolean
}

const { t, locale } = useI18n()
const route = useRoute()
const router = useRouter()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const live = useLive()

const moreOpen = ref(false)
const channelOpen = ref(false)
const moreMenu = ref<HTMLElement>()
const channelMenu = ref<HTMLElement>()

onClickOutside(moreMenu, () => (moreOpen.value = false))
onClickOutside(channelMenu, () => (channelOpen.value = false))

const items = computed<NavItem[]>(() => {
    const all: NavItem[] = [
        { key: 'dashboard', label: t('nav.dashboard'), icon: 'bi-speedometer2', path: '/', main: true },
        { key: 'onair', label: t('nav.onair'), icon: 'bi-broadcast', path: '/player', main: true },
        { key: 'commercial', label: t('nav.commercial'), icon: 'bi-megaphone', path: '/commercial', main: true },
        { key: 'files', label: t('nav.files'), icon: 'bi-folder2-open', path: '/media', main: true },
        { key: 'cg', label: t('nav.cg'), icon: 'bi-layers', path: '/cg' },
        { key: 'message', label: t('nav.message'), icon: 'bi-chat-square-text', path: '/message' },
        { key: 'history', label: t('nav.history'), icon: 'bi-clock-history', path: '/logging' },
        {
            key: 'settings',
            label: t('nav.settings'),
            icon: 'bi-gear',
            path: '/configure',
            roles: ['global_admin', 'channel_admin'],
        },
    ]

    return all.filter((item) => !item.roles || item.roles.includes(authStore.role))
})

const mainItems = computed(() => items.value.filter((item) => item.main))
const moreItems = computed(() => items.value.filter((item) => !item.main))

const pill = computed<{ kind: StatusKind; label: string }>(() => {
    const kinds: Record<ChannelState, StatusKind> = {
        on_air: 'ok',
        live: 'live',
        restarting: 'warn',
        stopped: 'bad',
        offline: 'idle',
    }

    return { kind: kinds[live.state], label: t(`live.${live.state}`) }
})

const channelId = computed(() => configStore.channels[configStore.i]?.id)
const channelName = computed(() => configStore.channels[configStore.i]?.name ?? '')

function channelLink(path: string) {
    return channelId.value ? { path, query: { channel: channelId.value } } : path
}

function isActive(item: NavItem) {
    return item.path === '/' ? route.path === '/' : route.path.startsWith(item.path)
}

async function selectChannel(id: number) {
    channelOpen.value = false
    await router.push({ path: route.path, query: { ...route.query, channel: id } })
}

function toggleTheme() {
    indexStore.darkMode = !indexStore.darkMode

    if (indexStore.darkMode) {
        localStorage.setItem('theme', 'dark')
        document.documentElement.setAttribute('data-theme', 'ffp-dark')
    } else {
        localStorage.setItem('theme', 'light')
        document.documentElement.setAttribute('data-theme', 'ffp-light')
    }
}

function changeLanguage(code: string) {
    locale.value = code
    localStorage.setItem('language', code)
}

async function logout() {
    live.stop()
    await authStore.logout()
    await router.push({ name: 'login' })
}

watch(
    channelId,
    (id) => {
        if (id) {
            void live.start(id)
        }
    },
    { immediate: true },
)
onMounted(() => {
    if (channelId.value) {
        void live.start(channelId.value)
    }
})
</script>
<template>
    <nav class="bg-base-100 shadow-md" aria-label="Menu">
        <div class="flex items-center gap-2 min-h-14 px-2 sm:px-4">
            <RouterLink :to="channelLink('/')" class="shrink-0 p-1" :title="t('nav.dashboard')">
                <img src="@/assets/images/ffplayout-small.png" alt="Logo" width="32" height="32" />
            </RouterLink>

            <!-- Menu do computador -->
            <ul class="hidden md:flex items-center gap-1 ms-2">
                <li v-for="item in items" :key="item.key">
                    <RouterLink
                        :to="channelLink(item.path)"
                        class="flex items-center gap-2 rounded-lg px-3 py-2 font-medium transition-colors hover:bg-base-200"
                        :class="isActive(item) ? 'bg-primary/15 text-primary font-semibold' : ''"
                    >
                        <i class="bi" :class="item.icon" />
                        {{ item.label }}
                    </RouterLink>
                </li>
            </ul>

            <div class="grow" />

            <!-- Canal -->
            <div v-if="configStore.channels.length > 1" ref="channelMenu" class="relative">
                <button class="btn btn-sm" :aria-label="t('nav.channel')" @click="channelOpen = !channelOpen">
                    <i class="bi bi-tv" />
                    <span class="max-w-32 truncate">{{ channelName }}</span>
                    <i class="bi bi-chevron-down text-xs" />
                </button>
                <ul
                    v-if="channelOpen"
                    class="absolute right-0 mt-1 z-40 w-56 rounded-xl bg-base-100 p-2 shadow-xl border border-base-300"
                >
                    <li v-for="channel in configStore.channels" :key="channel.id">
                        <button
                            class="w-full text-left rounded-lg px-3 py-2 hover:bg-base-200"
                            :class="channel.id === channelId && 'font-bold'"
                            @click="selectChannel(channel.id)"
                        >
                            {{ channel.name }}
                        </button>
                    </li>
                </ul>
            </div>

            <!-- Estado do canal, sempre visível -->
            <RouterLink :to="channelLink('/')" :title="t(`live.${live.state}_help`)">
                <StatusPill :kind="pill.kind" :label="pill.label" />
            </RouterLink>

            <!-- Tema, idioma e sair -->
            <div ref="moreMenu" class="relative">
                <button
                    class="btn btn-sm btn-circle btn-ghost text-lg"
                    :aria-label="t('nav.menu')"
                    :aria-expanded="moreOpen"
                    @click="moreOpen = !moreOpen"
                >
                    <i class="bi" :class="moreOpen ? 'bi-x-lg' : 'bi-three-dots-vertical'" />
                </button>
                <div
                    v-if="moreOpen"
                    class="absolute right-0 mt-1 z-40 w-64 rounded-xl bg-base-100 p-2 shadow-xl border border-base-300"
                >
                    <RouterLink
                        v-for="item in moreItems"
                        :key="item.key"
                        :to="channelLink(item.path)"
                        class="flex md:hidden items-center gap-3 rounded-lg px-3 py-3 hover:bg-base-200"
                        @click="moreOpen = false"
                    >
                        <i class="bi w-5 text-center" :class="item.icon" /> {{ item.label }}
                    </RouterLink>
                    <RouterLink
                        :to="channelLink('/configure/user')"
                        class="flex items-center gap-3 rounded-lg px-3 py-3 hover:bg-base-200"
                        @click="moreOpen = false"
                    >
                        <i class="bi bi-person-circle w-5 text-center" /> {{ t('config.user') }}
                    </RouterLink>
                    <button
                        class="w-full flex items-center gap-3 rounded-lg px-3 py-3 hover:bg-base-200"
                        @click="toggleTheme"
                    >
                        <i class="bi w-5 text-center" :class="indexStore.darkMode ? 'bi-brightness-high' : 'bi-moon'" />
                        {{ t('nav.theme') }}
                    </button>
                    <div class="px-3 pt-2 pb-1 text-xs uppercase tracking-wide opacity-60">{{ t('nav.language') }}</div>
                    <button
                        v-for="lang in locales"
                        :key="lang.code"
                        class="w-full text-left rounded-lg px-3 py-2 hover:bg-base-200"
                        :class="lang.code === locale && 'font-bold'"
                        @click="changeLanguage(lang.code)"
                    >
                        {{ lang.name }}
                    </button>
                    <div class="my-1 border-t border-base-300" />
                    <button
                        class="w-full flex items-center gap-3 rounded-lg px-3 py-3 hover:bg-base-200"
                        @click="logout"
                    >
                        <i class="bi bi-door-closed w-5 text-center" /> {{ t('nav.logout') }}
                    </button>
                </div>
            </div>
        </div>
    </nav>

    <!-- Barra de baixo do celular -->
    <nav
        class="md:hidden fixed bottom-0 inset-x-0 z-30 bg-base-100 border-t border-base-300 grid"
        :style="{ gridTemplateColumns: `repeat(${mainItems.length}, minmax(0, 1fr))` }"
        aria-label="Menu"
    >
        <RouterLink
            v-for="item in mainItems"
            :key="item.key"
            :to="channelLink(item.path)"
            class="flex flex-col items-center gap-0.5 py-2 text-xs"
            :class="isActive(item) ? 'text-primary font-semibold' : 'opacity-70'"
        >
            <i class="bi text-xl" :class="item.icon" />
            {{ item.label }}
        </RouterLink>
    </nav>
</template>
