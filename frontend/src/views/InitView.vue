<script setup lang="ts">
import { computed, nextTick, onMounted, ref, useTemplateRef } from 'vue'
import { useHead } from '@unhead/vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'

import AuthShell from '@/components/ui/AuthShell.vue'
import HelpTip from '@/components/ui/HelpTip.vue'
import PasswordField from '@/components/ui/PasswordField.vue'

type PathKey = 'storage' | 'playlists' | 'public' | 'logs'

const PATH_KEYS: PathKey[] = ['storage', 'playlists', 'public', 'logs']

const { t } = useI18n()
const router = useRouter()

const loading = ref(true)
const submitting = ref(false)
const showAdvanced = ref(false)
const error = ref('')
const confirmPassword = ref('')
const suggested = ref({} as Partial<Record<PathKey, string>>)
const usernameInput = useTemplateRef<HTMLInputElement>('usernameInput')
const form = ref({
    logs: '',
    playlists: '',
    public: '',
    storage: '',
    shared: false,
    smtp_server: '',
    smtp_user: '',
    smtp_password: '',
    smtp_starttls: false,
    smtp_port: 465,
    username: '',
    mail: '',
    password: '',
    two_factor: true,
})

const passwordMismatch = computed(() => confirmPassword.value !== '' && form.value.password !== confirmPassword.value)
const twoFactorWithoutMail = computed(() => form.value.two_factor && !form.value.smtp_server.trim())

useHead({ title: computed(() => t('auth2.init.title')) })

onMounted(loadSetup)

async function loadSetup() {
    try {
        const response = await fetch('/api/setup')
        const data = await response.json()

        if (!response.ok || !data.required) {
            await router.replace({ name: 'login' })
            return
        }

        Object.assign(form.value, data.settings)

        // Remember what the system proposed so the operator can always go back to it.
        for (const key of PATH_KEYS) {
            suggested.value[key] = form.value[key]
        }
    } catch {
        error.value = t('setup.loadFailed')
    } finally {
        loading.value = false
    }

    await nextTick()
    usernameInput.value?.focus()
}

async function submit() {
    if (form.value.password !== confirmPassword.value) {
        error.value = t('setup.passwordMismatch')
        return
    }

    submitting.value = true
    error.value = ''

    try {
        const response = await fetch('/api/setup', {
            method: 'POST',
            headers: { 'content-type': 'application/json;charset=UTF-8' },
            body: JSON.stringify(form.value),
        })

        if (!response.ok) {
            throw new Error(await response.text())
        }

        await router.replace({ name: 'login' })
    } catch (cause) {
        error.value = cause instanceof Error ? cause.message : t('setup.saveFailed')
    } finally {
        submitting.value = false
    }
}
</script>

<template>
    <AuthShell wide :title="t('auth2.init.heading')" :subtitle="t('auth2.init.subtitle')">
        <div v-if="loading" class="flex justify-center py-10">
            <span class="loading loading-spinner loading-lg text-primary" />
        </div>

        <form v-else class="flex flex-col gap-8" @submit.prevent="submit">
            <!-- 1. Administrator -->
            <section>
                <h2 class="flex items-center gap-3 text-xl font-bold">
                    <span class="flex h-8 w-8 items-center justify-center rounded-full bg-primary text-primary-content"
                        >1</span
                    >
                    {{ t('auth2.init.adminTitle') }}
                </h2>
                <p class="mt-1 opacity-70">{{ t('auth2.init.adminText') }}</p>

                <div class="mt-4 grid gap-4 sm:grid-cols-2">
                    <div>
                        <label class="block text-sm font-semibold mb-1" for="init-username">
                            {{ t('auth2.init.username') }}
                        </label>
                        <input
                            id="init-username"
                            ref="usernameInput"
                            v-model="form.username"
                            name="username"
                            type="text"
                            autocomplete="username"
                            autocapitalize="off"
                            :placeholder="t('auth2.init.usernamePlaceholder')"
                            class="input input-lg w-full"
                            required
                        />
                        <p class="mt-1 text-sm opacity-70">{{ t('auth2.init.usernameHelp') }}</p>
                    </div>
                    <div>
                        <label class="block text-sm font-semibold mb-1" for="init-mail">
                            {{ t('auth2.init.mail') }}
                        </label>
                        <input
                            id="init-mail"
                            v-model="form.mail"
                            name="mail"
                            type="email"
                            autocomplete="email"
                            :placeholder="t('auth2.init.mailPlaceholder')"
                            class="input input-lg w-full"
                            required
                        />
                        <p class="mt-1 text-sm opacity-70">{{ t('auth2.init.mailHelp') }}</p>
                    </div>
                    <PasswordField
                        v-model="form.password"
                        name="new-password"
                        autocomplete="new-password"
                        :label="t('auth2.init.password')"
                        :hint="t('auth2.init.passwordHelp')"
                    />
                    <PasswordField
                        v-model="confirmPassword"
                        name="confirm-password"
                        autocomplete="new-password"
                        :label="t('auth2.init.confirmPassword')"
                        :invalid="passwordMismatch"
                        :hint="passwordMismatch ? t('setup.passwordMismatch') : t('auth2.init.confirmHelp')"
                    />
                </div>

                <label class="mt-4 flex items-start gap-3 cursor-pointer">
                    <input v-model="form.two_factor" type="checkbox" class="toggle toggle-primary mt-0.5" />
                    <span>
                        <span class="block font-semibold">{{ t('auth2.init.twoFactor') }}</span>
                        <span class="block text-sm opacity-70">{{ t('auth2.init.twoFactorHelp') }}</span>
                    </span>
                </label>
                <div v-if="twoFactorWithoutMail" class="alert alert-warning mt-3 text-sm">
                    <i class="bi bi-exclamation-triangle-fill text-lg" />
                    <span>{{ t('auth2.init.twoFactorWarning') }}</span>
                </div>
            </section>

            <!-- 2. Folders -->
            <section>
                <h2 class="flex items-center gap-3 text-xl font-bold">
                    <span class="flex h-8 w-8 items-center justify-center rounded-full bg-primary text-primary-content"
                        >2</span
                    >
                    {{ t('auth2.init.foldersTitle') }}
                </h2>
                <p class="mt-1 opacity-70">{{ t('auth2.init.foldersText') }}</p>

                <div class="mt-4 flex flex-col gap-4">
                    <div v-for="key in PATH_KEYS" :key="key">
                        <label class="flex items-center gap-1 text-sm font-semibold mb-1" :for="`init-${key}`">
                            {{ t(`auth2.init.paths.${key}.label`) }}
                            <HelpTip :text="t(`auth2.init.paths.${key}.tip`)" />
                        </label>
                        <input
                            :id="`init-${key}`"
                            v-model="form[key]"
                            :name="key"
                            type="text"
                            class="input w-full font-mono"
                            autocapitalize="off"
                            spellcheck="false"
                            required
                        />
                        <p class="mt-1 text-sm opacity-70">{{ t(`auth2.init.paths.${key}.help`) }}</p>
                        <p v-if="suggested[key] && form[key] !== suggested[key]" class="mt-1 text-sm">
                            {{ t('auth2.init.suggested') }}
                            <code class="font-mono">{{ suggested[key] }}</code>
                            <button type="button" class="btn btn-xs ms-2" @click="form[key] = suggested[key] ?? ''">
                                {{ t('auth2.init.useSuggested') }}
                            </button>
                        </p>
                    </div>

                    <label class="flex items-start gap-3 cursor-pointer">
                        <input v-model="form.shared" type="checkbox" class="checkbox mt-0.5" />
                        <span>
                            <span class="block font-semibold">{{ t('auth2.init.shared') }}</span>
                            <span class="block text-sm opacity-70">{{ t('auth2.init.sharedHelp') }}</span>
                        </span>
                    </label>
                </div>
            </section>

            <!-- 3. Email sending (advanced) -->
            <section>
                <button
                    type="button"
                    class="flex w-full items-center gap-3 text-left text-xl font-bold cursor-pointer"
                    :aria-expanded="showAdvanced"
                    @click="showAdvanced = !showAdvanced"
                >
                    <span class="flex h-8 w-8 items-center justify-center rounded-full bg-base-300">3</span>
                    <span class="grow">{{ t('auth2.init.mailTitle') }}</span>
                    <i class="bi text-base" :class="showAdvanced ? 'bi-chevron-up' : 'bi-chevron-down'" />
                </button>
                <p class="mt-1 opacity-70">{{ t('auth2.init.mailText') }}</p>

                <div v-if="showAdvanced" class="mt-4 rounded-xl bg-base-200 p-4">
                    <p class="mb-3 text-sm opacity-70">{{ t('ui.advancedHelp') }}</p>
                    <div class="grid gap-4 sm:grid-cols-2">
                        <div>
                            <label class="block text-sm font-semibold mb-1" for="init-smtp-server">
                                {{ t('auth2.init.smtp.server') }}
                            </label>
                            <input
                                id="init-smtp-server"
                                v-model="form.smtp_server"
                                name="smtp_server"
                                type="text"
                                class="input w-full"
                                placeholder="smtp.exemplo.com.br"
                            />
                            <p class="mt-1 text-sm opacity-70">{{ t('auth2.init.smtp.serverHelp') }}</p>
                        </div>
                        <div>
                            <label class="block text-sm font-semibold mb-1" for="init-smtp-port">
                                {{ t('auth2.init.smtp.port') }}
                            </label>
                            <input
                                id="init-smtp-port"
                                v-model.number="form.smtp_port"
                                type="number"
                                min="1"
                                max="65535"
                                class="input w-full"
                            />
                            <p class="mt-1 text-sm opacity-70">{{ t('auth2.init.smtp.portHelp') }}</p>
                        </div>
                        <div>
                            <label class="block text-sm font-semibold mb-1" for="init-smtp-user">
                                {{ t('auth2.init.smtp.user') }}
                            </label>
                            <input
                                id="init-smtp-user"
                                v-model="form.smtp_user"
                                name="smtp_user"
                                type="text"
                                class="input w-full"
                                autocapitalize="off"
                            />
                        </div>
                        <PasswordField
                            v-model="form.smtp_password"
                            name="smtp-password"
                            autocomplete="new-password"
                            :label="t('auth2.init.smtp.password')"
                            :required="false"
                        />
                    </div>
                    <label class="mt-4 flex items-start gap-3 cursor-pointer">
                        <input v-model="form.smtp_starttls" type="checkbox" class="checkbox mt-0.5" />
                        <span>
                            <span class="block font-semibold">{{ t('auth2.init.smtp.starttls') }}</span>
                            <span class="block text-sm opacity-70">{{ t('auth2.init.smtp.starttlsHelp') }}</span>
                        </span>
                    </label>
                </div>
            </section>

            <div v-if="error" role="alert" class="alert alert-error">
                <i class="bi bi-exclamation-triangle-fill text-xl" />
                <span class="font-semibold break-words">{{ error }}</span>
            </div>

            <div>
                <button type="submit" class="btn btn-primary btn-lg w-full sm:w-auto" :disabled="submitting">
                    <span v-if="submitting" class="loading loading-spinner" />
                    <i v-else class="bi bi-check-lg" />
                    {{ t('auth2.init.finish') }}
                </button>
                <p class="mt-2 text-sm opacity-70">{{ t('auth2.init.finishHelp') }}</p>
            </div>
        </form>
    </AuthShell>
</template>
