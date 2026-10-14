<script setup lang="ts">
import { computed, onMounted, ref, useTemplateRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { useHead } from '@unhead/vue'
import { useRouter } from 'vue-router'

import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'

import AuthShell from '@/components/ui/AuthShell.vue'
import PasswordField from '@/components/ui/PasswordField.vue'

const { t } = useI18n()
const authStore = useAuth()
const indexStore = useIndex()
const router = useRouter()

const formError = ref('')
const formPassword = ref('')
const disabled = ref(false)
const usernameInput = useTemplateRef<HTMLInputElement>('usernameInput')
const passwordField = useTemplateRef<InstanceType<typeof PasswordField>>('passwordField')

useHead({
    title: computed(() => t('auth2.loginTitle')),
})

onMounted(() => {
    // Returning users already have their name filled in: go straight to the password.
    if (authStore.username) {
        passwordField.value?.focus()
    } else {
        usernameInput.value?.focus()
    }
})

function errorFor(status: number): string {
    if (status === 429) {
        return t('auth2.tooManyTries')
    }

    return status >= 500 ? t('auth2.serverError') : t('auth2.wrongLogin')
}

async function login() {
    if (disabled.value) {
        return
    }

    disabled.value = true
    formError.value = ''

    try {
        const result = await authStore.obtainVerificationCode(formPassword.value)

        formPassword.value = ''

        if (result.status !== 200) {
            formError.value = errorFor(result.status)
            disabled.value = false
            passwordField.value?.focus()

            return
        }

        if (result.verificationRequired) {
            authStore.beginVerification()
            indexStore.msgAlert('success', t('alert.verificationSent'))
            await router.push({ name: 'verification' })
            disabled.value = false
            return
        }

        await router.replace({ name: 'home' })
        disabled.value = false
    } catch {
        disabled.value = false
        formError.value = t('auth2.serverError')
    }
}
</script>
<template>
    <AuthShell :title="t('auth2.welcome')" :subtitle="t('auth2.welcomeText')">
        <form class="flex flex-col gap-5" @submit.prevent="login">
            <div>
                <label class="block text-sm font-semibold mb-1" for="login-username">{{ t('auth2.username') }}</label>
                <input
                    id="login-username"
                    ref="usernameInput"
                    v-model="authStore.username"
                    type="text"
                    name="username"
                    autocomplete="username"
                    autocapitalize="off"
                    spellcheck="false"
                    :placeholder="t('auth2.usernamePlaceholder')"
                    class="input input-lg w-full"
                    :class="formError && 'input-error'"
                    required
                />
            </div>

            <PasswordField
                ref="passwordField"
                v-model="formPassword"
                :label="t('auth2.password')"
                :placeholder="t('auth2.passwordPlaceholder')"
                :invalid="Boolean(formError)"
            />

            <div v-if="formError" role="alert" class="alert alert-error text-base">
                <i class="bi bi-exclamation-triangle-fill text-xl" />
                <span class="font-semibold">{{ formError }}</span>
            </div>

            <button type="submit" class="btn btn-primary btn-lg w-full" :disabled="disabled">
                <span v-if="disabled" class="loading loading-spinner" />
                {{ disabled ? t('auth2.entering') : t('auth2.enter') }}
            </button>
        </form>

        <template #footer>{{ t('auth2.forgotHelp') }}</template>
    </AuthShell>
</template>
