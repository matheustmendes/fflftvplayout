<script setup lang="ts">
import { computed, ref, onMounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { storeToRefs } from 'pinia'
import dayjs from 'dayjs'
import { isEqual } from 'es-toolkit/predicate'
import { cloneDeep } from 'es-toolkit/object'

import AdvancedBlock from '@/components/ui/AdvancedBlock.vue'
import ConfirmModal from '@/components/ui/ConfirmModal.vue'
import SaveBar from '@/components/ui/SaveBar.vue'
import RestartModal from '@/components/config/RestartModal.vue'
import SettingsCard from '@/components/config/SettingsCard.vue'
import SettingsField from '@/components/config/SettingsField.vue'
import SettingsGroup from '@/components/config/SettingsGroup.vue'

import { authFetch } from '@/composables/authFetch'
import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'
import { useConfig } from '@/stores/config'
import { useMedia } from '@/stores/media'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const authStore = useAuth()
const configStore = useConfig()
const mediaStore = useMedia()
const indexStore = useIndex()
const { i } = storeToRefs(useConfig())

const saving = ref(false)
const showDeleteModal = ref(false)
const channel = ref({} as Channel)
const channelOrig = ref({} as Channel)

const dirty = computed(() => !isEqual(channel.value, channelOrig.value))
const isOperator = computed(() => authStore.role === 'user')
const isGlobalAdmin = computed(() => authStore.role === 'global_admin')
const canDelete = computed(
    () => isGlobalAdmin.value && configStore.channels.length > 1 && channel.value.id > 1 && !dirty.value,
)

function loadSelectedChannel() {
    channel.value = cloneDeep(configStore.channels[i.value] ?? ({} as Channel))
    channelOrig.value = cloneDeep(configStore.channels[i.value] ?? ({} as Channel))
}

onMounted(loadSelectedChannel)

watch([i], () => {
    if (configStore.channels[i.value]) {
        loadSelectedChannel()
    }
})

function rmId(path: string) {
    return path.replace(/\/\d+$/, '')
}

function newChannel() {
    channel.value.id = configStore.channels.length + 1
    channel.value.name = `Channel ${channel.value.id}`
    channel.value.preview_url = `${window.location.protocol}//${window.location.host}/public/${channel.value.id}/live/master.m3u8`
    channel.value.public = `${rmId(channel.value.public)}/${channel.value.id}`
    channel.value.playlists = `${rmId(channel.value.playlists)}/${channel.value.id}`
    channel.value.storage = `${rmId(channel.value.storage)}/${channel.value.id}`
    channel.value.timezone = dayjs.tz.guess()
}

async function addNewChannel() {
    await authFetch<Channel>('/api/channel', {
        method: 'POST',
        headers: { ...configStore.contentType, ...authStore.authHeader },
        body: JSON.stringify(channel.value),
    })
        .then((chl) => {
            configStore.channels.push(cloneDeep(chl as Channel))
            configStore.channelsRaw.push(chl as Channel)
            configStore.configCount = configStore.channels.length
            configStore.timezone = channel.value.timezone || 'UTC'
            configStore.selectChannel(chl.id)
            void router.replace({ path: route.path, query: { ...route.query, channel: chl.id } })

            indexStore.msgAlert('success', t('settings2.channel.saved'), 2)
        })
        .catch(() => {
            indexStore.msgAlert('error', t('settings2.channel.saveFailed'), 3)
        })
}

async function updateChannel() {
    await authFetch(`/api/channel/${channel.value.id}`, {
        method: 'PATCH',
        headers: { ...configStore.contentType, ...authStore.authHeader },
        body: JSON.stringify(channel.value),
    })
        .then(() => {
            const oldTimezone = configStore.timezone
            const currentTimezone = channel.value.timezone

            for (let i = 0; i < configStore.channels.length; i++) {
                if (configStore.channels[i]?.id === channel.value.id) {
                    configStore.channels[i] = cloneDeep(channel.value)
                    configStore.timezone = channel.value.timezone || 'UTC'
                    break
                }
            }

            for (let i = 0; i < configStore.channelsRaw.length; i++) {
                if (configStore.channelsRaw[i]?.id === channel.value.id) {
                    configStore.channelsRaw[i] = cloneDeep(channel.value)
                    break
                }
            }

            loadSelectedChannel()

            if (oldTimezone !== currentTimezone) {
                configStore.showRestartModal = true
            }

            indexStore.msgAlert('success', t('settings2.channel.saved'), 2)
        })
        .catch(() => {
            indexStore.msgAlert('error', t('settings2.channel.saveFailed'), 3)
        })
}

async function addUpdateChannel() {
    /*
        Save or update channel settings.
    */
    if (!dirty.value || saving.value) {
        return
    }

    saving.value = true

    try {
        if (configStore.channels[i.value]?.id !== channel.value.id) {
            await addNewChannel()
        } else {
            await updateChannel()
        }

        await configStore.getPlayoutConfig()
        await configStore.getPlayoutOutputs()
        await configStore.getUserConfig()
        await mediaStore.getTree('')
    } finally {
        saving.value = false
    }
}

function resetChannel() {
    channel.value = cloneDeep(configStore.channels[i.value] ?? ({} as Channel))
}

async function deleteChannel() {
    showDeleteModal.value = false

    if (channel.value.id === 1) {
        indexStore.msgAlert('warning', t('settings2.channel.firstCannotDelete'), 2)
        return
    }

    try {
        await authFetch(`/api/channel/${channel.value.id}`, {
            method: 'DELETE',
            headers: authStore.authHeader,
        })
    } catch {
        indexStore.msgAlert('error', t('settings2.channel.deleteFailed'), 2)
        return
    }

    await configStore.getChannelConfig()
    const previousChannel = configStore.channels[Math.max(0, i.value - 1)]
    configStore.selectChannel(previousChannel?.id)
    await configStore.getPlayoutConfig()
    await configStore.getPlayoutOutputs()
    await configStore.getUserConfig()
    await mediaStore.getTree('')

    const selectedChannel = configStore.channels[configStore.i]?.id
    if (selectedChannel) {
        await router.replace({ path: route.path, query: { ...route.query, channel: selectedChannel } })
    }

    indexStore.msgAlert('success', t('settings2.channel.deleted'), 2)
}
</script>
<template>
    <div v-if="channel" class="space-y-5">
        <form class="space-y-5" @submit.prevent="addUpdateChannel">
            <SettingsCard
                icon="bi-tv"
                :title="t('settings2.channel.cardTitle')"
                :text="t('settings2.channel.cardText')"
            >
                <template v-if="isGlobalAdmin" #actions>
                    <button type="button" class="btn btn-sm btn-outline" @click="newChannel()">
                        <i class="bi bi-plus-lg" />
                        {{ t('settings2.channel.newChannel') }}
                    </button>
                </template>
                <SettingsField :label="t('settings2.channel.name')" :help="t('settings2.channel.nameHelp')">
                    <input v-model="channel.name" type="text" name="name" class="input w-full" :disabled="isOperator" />
                </SettingsField>
                <p v-if="isOperator" class="text-sm opacity-70">
                    <i class="bi bi-lock me-1" />
                    {{ t('settings2.channel.readOnly') }}
                </p>
            </SettingsCard>

            <AdvancedBlock v-if="!isOperator">
                <SettingsGroup
                    :title="t('settings2.channel.advancedPaths')"
                    :text="t('settings2.channel.advancedPathsHelp')"
                >
                    <SettingsField :label="t('settings2.channel.previewUrl')">
                        <input v-model="channel.preview_url" type="text" name="preview_url" class="input w-full" />
                    </SettingsField>
                    <SettingsField
                        :label="t('settings2.channel.extensions')"
                        :hint="t('settings2.channel.extensionsHelp')"
                    >
                        <input
                            v-model="channel.extra_extensions"
                            type="text"
                            name="extra_extensions"
                            class="input w-full"
                        />
                    </SettingsField>
                    <template v-if="isGlobalAdmin">
                        <p
                            v-if="configStore.playout.storage?.shared_storage"
                            class="text-sm font-semibold text-warning"
                        >
                            <i class="bi bi-exclamation-triangle-fill me-1" />
                            {{ t('config.sharedStorage') }}
                        </p>
                        <SettingsField :label="t('settings2.channel.publicPath')">
                            <input v-model="channel.public" type="text" name="public" class="input w-full" />
                        </SettingsField>
                        <SettingsField :label="t('settings2.channel.playlistPath')">
                            <input v-model="channel.playlists" type="text" name="playlists" class="input w-full" />
                        </SettingsField>
                        <SettingsField :label="t('settings2.channel.storagePath')">
                            <input v-model="channel.storage" type="text" name="storage" class="input w-full" />
                        </SettingsField>
                        <SettingsField
                            :label="t('settings2.channel.timezone')"
                            :hint="t('settings2.channel.timezoneHelp')"
                        >
                            <select v-model="channel.timezone" class="select w-full max-w-xs">
                                <option v-for="zone in Intl.supportedValuesOf('timeZone')" :key="zone" :value="zone">
                                    {{ zone }}
                                </option>
                            </select>
                        </SettingsField>
                    </template>
                </SettingsGroup>
            </AdvancedBlock>

            <SaveBar v-if="!isOperator" :dirty="dirty" :saving="saving" require-change @discard="resetChannel" />
        </form>

        <SettingsCard
            v-if="canDelete"
            danger
            icon="bi-trash3"
            :title="t('settings2.channel.dangerTitle')"
            :text="t('settings2.channel.dangerText')"
        >
            <div>
                <button type="button" class="btn btn-error btn-outline" @click="showDeleteModal = true">
                    <i class="bi bi-trash3" />
                    {{ t('settings2.channel.deleteButton') }}
                </button>
            </div>
        </SettingsCard>

        <ConfirmModal
            :show="showDeleteModal"
            danger
            :title="t('settings2.channel.deleteConfirmTitle', { name: channel.name })"
            :text="t('settings2.channel.deleteConfirmText')"
            :confirm-label="t('settings2.channel.deleteConfirm')"
            @confirm="deleteChannel"
            @cancel="showDeleteModal = false"
        />
        <RestartModal />
    </div>
</template>
