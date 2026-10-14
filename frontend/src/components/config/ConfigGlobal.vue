<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { cloneDeep } from 'es-toolkit/object'
import { isEqual } from 'es-toolkit/predicate'
import { useI18n } from 'vue-i18n'

import AdvancedBlock from '@/components/ui/AdvancedBlock.vue'
import SaveBar from '@/components/ui/SaveBar.vue'
import SettingsCard from '@/components/config/SettingsCard.vue'
import SettingsField from '@/components/config/SettingsField.vue'
import SettingsGroup from '@/components/config/SettingsGroup.vue'

import { authFetch } from '@/composables/authFetch'
import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'

const { t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const settings = ref({} as GlobalSettings)
const savedSettings = ref({} as GlobalSettings)
const smtpPassword = ref('')
const notificationToken = ref('')
const loading = ref(true)
const saving = ref(false)

const dirty = computed(
    () =>
        smtpPassword.value.length > 0 ||
        notificationToken.value.length > 0 ||
        !isEqual(settings.value, savedSettings.value),
)

onMounted(getSettings)

async function getSettings() {
    loading.value = true

    try {
        settings.value = await authFetch<GlobalSettings>('/api/global', {
            headers: authStore.authHeader,
        })
        savedSettings.value = cloneDeep(settings.value)
        smtpPassword.value = ''
        notificationToken.value = ''
    } catch {
        indexStore.msgAlert('error', t('settings2.system.loadFailed'), 3)
    } finally {
        loading.value = false
    }
}

async function save() {
    if (saving.value) {
        return
    }

    saving.value = true

    try {
        settings.value = await authFetch<GlobalSettings>('/api/global', {
            method: 'PUT',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({
                smtp_server: settings.value.smtp_server,
                smtp_user: settings.value.smtp_user,
                smtp_password: smtpPassword.value,
                smtp_starttls: settings.value.smtp_starttls,
                smtp_port: settings.value.smtp_port,
                notification_server: settings.value.notification_server,
                notification_token: notificationToken.value,
            }),
        })

        savedSettings.value = cloneDeep(settings.value)
        smtpPassword.value = ''
        notificationToken.value = ''
        await configStore.getPlayoutConfig()
        indexStore.msgAlert('success', t('settings2.system.saved'), 2)
    } catch {
        indexStore.msgAlert('error', t('settings2.system.saveFailed'), 3)
    } finally {
        saving.value = false
    }
}
</script>

<template>
    <div v-if="authStore.role === 'global_admin'">
        <p v-if="loading" class="py-10 text-center opacity-70">
            <span class="loading loading-spinner loading-md me-2" />
            {{ t('ui.loading') }}
        </p>
        <form v-else class="space-y-5" @submit.prevent="save">
            <p class="text-sm opacity-70">{{ t('settings2.system.intro') }}</p>

            <SettingsCard
                icon="bi-envelope"
                :title="t('settings2.system.mailTitle')"
                :text="t('settings2.system.mailText')"
            >
                <SettingsField :label="t('settings2.system.server')" :help="t('settings2.system.serverHelp')">
                    <input v-model="settings.smtp_server" type="text" class="input w-full" name="smtp_server" />
                </SettingsField>
                <div class="grid gap-4 sm:grid-cols-2">
                    <SettingsField :label="t('settings2.system.user')">
                        <input v-model="settings.smtp_user" type="text" class="input w-full" name="smtp_user" />
                    </SettingsField>
                    <SettingsField :label="t('settings2.system.password')">
                        <input
                            v-model="smtpPassword"
                            type="password"
                            class="input w-full"
                            autocomplete="new-password"
                            :placeholder="
                                settings.smtp_password_set
                                    ? t('settings2.system.passwordSet')
                                    : t('settings2.system.passwordPlaceholder')
                            "
                        />
                    </SettingsField>
                </div>
            </SettingsCard>

            <SettingsCard
                icon="bi-phone-vibrate"
                :title="t('settings2.system.pushTitle')"
                :text="t('settings2.system.pushText')"
            >
                <SettingsField :label="t('settings2.system.pushServer')" :help="t('settings2.system.pushServerHelp')">
                    <input
                        v-model="settings.notification_server"
                        type="url"
                        name="notification_server"
                        placeholder="https://push.example.org"
                        class="input w-full"
                    />
                </SettingsField>
                <SettingsField :label="t('settings2.system.pushToken')">
                    <input
                        v-model="notificationToken"
                        type="password"
                        class="input w-full"
                        autocomplete="new-password"
                        :placeholder="
                            settings.notification_token_set
                                ? t('settings2.system.tokenSet')
                                : t('settings2.system.tokenPlaceholder')
                        "
                    />
                </SettingsField>
            </SettingsCard>

            <AdvancedBlock>
                <SettingsGroup :title="t('config.smtp')">
                    <div class="grid gap-3 sm:grid-cols-2">
                        <SettingsField :label="t('settings2.system.port')">
                            <input
                                v-model.number="settings.smtp_port"
                                type="number"
                                min="1"
                                max="65535"
                                class="input input-sm w-full"
                            />
                        </SettingsField>
                    </div>
                    <label class="flex cursor-pointer items-center gap-2">
                        <input v-model="settings.smtp_starttls" type="checkbox" class="checkbox" />
                        {{ t('settings2.system.starttls') }}
                    </label>
                </SettingsGroup>
            </AdvancedBlock>

            <SaveBar :dirty="dirty" :saving="saving" require-change @discard="getSettings" />
        </form>
    </div>
</template>
