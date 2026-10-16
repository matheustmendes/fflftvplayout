<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import ConfirmModal from '@/components/ui/ConfirmModal.vue'
import SaveBar from '@/components/ui/SaveBar.vue'
import SettingsCard from '@/components/config/SettingsCard.vue'
import SettingsField from '@/components/config/SettingsField.vue'

import { friendlyError } from '@/composables/friendlyError'
import { telegramApi, type TelegramSettings } from '@/composables/telegramApi'
import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'

const { t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const channel = computed(() => configStore.channels[configStore.i]?.id)
const isGlobalAdmin = computed(() => authStore.role === 'global_admin')

const settings = ref<TelegramSettings | null>(null)
const saved = ref('')
const tokenSet = ref(false)
const tokenInput = ref('')
const saving = ref(false)
const testing = ref(false)
const showRemoveToken = ref(false)

const dirty = computed(() => settings.value !== null && JSON.stringify(settings.value) !== saved.value)

const alerts = [
    { key: 'alert_output', label: 'output', icon: 'bi-broadcast' },
    { key: 'alert_destinations', label: 'destinations', icon: 'bi-diagram-3' },
    { key: 'alert_filler', label: 'filler', icon: 'bi-file-earmark-play' },
    { key: 'alert_playlist', label: 'playlist', icon: 'bi-calendar-x' },
    { key: 'alert_commercials', label: 'commercials', icon: 'bi-megaphone' },
    { key: 'alert_disk', label: 'disk', icon: 'bi-hdd' },
] as const

function fail(error: unknown) {
    indexStore.msgAlert('error', friendlyError(error), 6)
}

async function load() {
    if (channel.value === undefined) {
        return
    }

    try {
        const view = await telegramApi.get(channel.value)

        settings.value = view.settings
        saved.value = JSON.stringify(view.settings)
        tokenSet.value = view.token_set
    } catch (error) {
        fail(error)
    }
}

async function save() {
    if (!settings.value || channel.value === undefined || saving.value) {
        return
    }

    saving.value = true

    try {
        const view = await telegramApi.save(channel.value, settings.value)

        settings.value = view.settings
        saved.value = JSON.stringify(view.settings)
        tokenSet.value = view.token_set
        indexStore.msgAlert('success', t('telegram.saved'), 3)
    } catch (error) {
        fail(error)
    } finally {
        saving.value = false
    }
}

function discard() {
    if (saved.value) {
        settings.value = JSON.parse(saved.value) as TelegramSettings
    }
}

async function saveToken(token: string) {
    if (channel.value === undefined) {
        return
    }

    try {
        const view = await telegramApi.saveToken(channel.value, token)

        tokenSet.value = view.token_set
        tokenInput.value = ''
        indexStore.msgAlert('success', t(token ? 'telegram.token.saved' : 'telegram.token.removed'), 3)
    } catch (error) {
        fail(error)
    }
}

async function removeToken() {
    showRemoveToken.value = false
    await saveToken('')
}

async function sendTest() {
    if (channel.value === undefined || testing.value) {
        return
    }

    testing.value = true

    try {
        const result = await telegramApi.test(channel.value)

        indexStore.msgAlert('success', t('telegram.testOk', { bot: result.bot }), 6)
    } catch (error) {
        fail(error)
    } finally {
        testing.value = false
    }
}

watch(channel, load, { immediate: true })
</script>

<template>
    <form v-if="settings" class="space-y-5" @submit.prevent="save">
        <SettingsCard icon="bi-telegram" :title="t('telegram.title')" :text="t('telegram.text')">
            <label class="flex cursor-pointer items-center gap-3">
                <input v-model="settings.enabled" type="checkbox" class="toggle toggle-primary" />
                <span class="font-semibold">{{ t('telegram.enable') }}</span>
                <span class="badge" :class="settings.enabled ? 'badge-success' : 'badge-ghost'">
                    {{ settings.enabled ? t('settings2.on') : t('settings2.off') }}
                </span>
            </label>
            <p class="text-sm opacity-70">{{ t('telegram.enableHelp') }}</p>

            <details class="rounded-lg bg-base-200 p-3" :open="!tokenSet || !settings.chat_id">
                <summary class="cursor-pointer font-semibold">{{ t('telegram.steps.title') }}</summary>
                <ol class="mt-2 list-decimal space-y-1 ps-5 text-sm">
                    <li>{{ t('telegram.steps.one') }}</li>
                    <li>{{ t('telegram.steps.two') }}</li>
                    <li>{{ t('telegram.steps.three') }}</li>
                    <li>{{ t('telegram.steps.four') }}</li>
                    <li>{{ t('telegram.steps.five') }}</li>
                </ol>
            </details>

            <SettingsField :label="t('telegram.token.label')" :help="t('telegram.token.help')">
                <div v-if="isGlobalAdmin" class="flex flex-wrap items-center gap-2">
                    <input
                        v-model.trim="tokenInput"
                        type="password"
                        autocomplete="off"
                        class="input w-full sm:w-96"
                        :placeholder="t('telegram.token.placeholder')"
                    />
                    <button
                        type="button"
                        class="btn btn-sm btn-primary"
                        :disabled="!tokenInput"
                        @click="saveToken(tokenInput)"
                    >
                        {{ t('telegram.token.save') }}
                    </button>
                    <button v-if="tokenSet" type="button" class="btn btn-sm btn-ghost" @click="showRemoveToken = true">
                        {{ t('telegram.token.remove') }}
                    </button>
                </div>
                <p v-else-if="!tokenSet" class="text-sm text-warning">{{ t('telegram.token.askAdmin') }}</p>
                <p
                    class="text-sm"
                    :class="tokenSet ? 'text-success font-semibold' : 'opacity-70'"
                    role="status"
                    data-testid="token-state"
                >
                    <i class="bi" :class="tokenSet ? 'bi-check-circle-fill' : 'bi-dash-circle'" />
                    {{ tokenSet ? t('telegram.token.isSet') : t('telegram.token.notSet') }}
                </p>
            </SettingsField>

            <SettingsField :label="t('telegram.chat.label')" :help="t('telegram.chat.help')">
                <input
                    v-model.trim="settings.chat_id"
                    type="text"
                    inputmode="numeric"
                    class="input w-full sm:w-72"
                    :placeholder="t('telegram.chat.placeholder')"
                />
            </SettingsField>

            <div class="flex flex-wrap items-center gap-3">
                <button
                    type="button"
                    class="btn btn-sm"
                    :disabled="dirty || testing || !tokenSet || !settings.chat_id"
                    @click="sendTest"
                >
                    <span v-if="testing" class="loading loading-spinner loading-sm" />
                    <i v-else class="bi bi-send" />
                    {{ t('telegram.test') }}
                </button>
                <span v-if="dirty" class="text-sm opacity-70">{{ t('telegram.testHelp') }}</span>
            </div>
        </SettingsCard>

        <SettingsCard icon="bi-bell" :title="t('telegram.alerts.title')" :text="t('telegram.alerts.text')">
            <div class="grid gap-3 sm:grid-cols-2">
                <label
                    v-for="alert in alerts"
                    :key="alert.key"
                    class="flex cursor-pointer items-start gap-3 rounded-lg bg-base-200 p-3"
                >
                    <input v-model="settings[alert.key]" type="checkbox" class="toggle toggle-sm toggle-primary mt-1" />
                    <span class="min-w-0">
                        <span class="block font-semibold">
                            <i class="bi me-1" :class="alert.icon" />{{ t(`telegram.alerts.${alert.label}`) }}
                        </span>
                        <span class="block text-sm opacity-70">{{ t(`telegram.alerts.${alert.label}Help`) }}</span>
                    </span>
                </label>
            </div>

            <div class="flex flex-wrap items-center gap-x-6 gap-y-2">
                <label class="flex cursor-pointer items-center gap-3">
                    <input v-model="settings.daily_summary" type="checkbox" class="toggle toggle-sm toggle-primary" />
                    <span class="font-semibold">{{ t('telegram.summary.label') }}</span>
                </label>
                <label v-if="settings.daily_summary" class="flex items-center gap-2">
                    <span>{{ t('telegram.summary.time') }}</span>
                    <input v-model="settings.summary_time" type="time" class="input input-sm w-28" />
                </label>
            </div>
            <p class="text-sm opacity-70">{{ t('telegram.summary.help') }}</p>
        </SettingsCard>

        <SettingsCard icon="bi-chat-dots" :title="t('telegram.commands.title')" :text="t('telegram.commands.text')">
            <label class="flex cursor-pointer items-center gap-3">
                <input v-model="settings.commands_enabled" type="checkbox" class="toggle toggle-primary" />
                <span class="font-semibold">{{ t('telegram.commands.enable') }}</span>
            </label>
            <p class="text-sm opacity-80">{{ t('telegram.commands.list') }}</p>
        </SettingsCard>

        <SaveBar :dirty="dirty" :saving="saving" require-change @discard="discard" />

        <ConfirmModal
            :show="showRemoveToken"
            danger
            :title="t('telegram.token.removeTitle')"
            :text="t('telegram.token.removeText')"
            :confirm-label="t('telegram.token.remove')"
            @confirm="removeToken"
            @cancel="showRemoveToken = false"
        />
    </form>
</template>
