<script setup lang="ts">
import { computed, onMounted, ref, useTemplateRef } from 'vue'
import { useHead } from '@unhead/vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { useAuth } from '@/stores/auth'

import AuthShell from '@/components/ui/AuthShell.vue'

const { t } = useI18n()
const router = useRouter()
const auth = useAuth()

const verificationCode = ref('')
const formError = ref('')
const submitting = ref(false)
const codeInput = useTemplateRef<HTMLInputElement>('codeInput')

useHead({
    title: computed(() => t('auth2.verifyTitle')),
})

onMounted(() => {
    codeInput.value?.focus()
})

async function verify() {
    if (submitting.value) {
        return
    }

    submitting.value = true
    formError.value = ''

    try {
        const status = await auth.verifyCode(verificationCode.value)

        verificationCode.value = ''

        if (status === 401 || status === 400 || status === 403) {
            formError.value = t('auth2.wrongCode')
            codeInput.value?.focus()
        }

        if (status === 200) {
            await auth.selectAuthUser()
            await router.push({ name: 'home' })
        }
    } catch {
        formError.value = t('auth2.serverError')
    }

    submitting.value = false
}
</script>
<template>
    <AuthShell v-if="!auth.isLogin" :title="t('auth2.verifyHeading')" :subtitle="t('auth2.verifyText')">
        <form class="flex flex-col gap-5" @submit.prevent="verify">
            <div>
                <label class="block text-sm font-semibold mb-1" for="verify-username">
                    {{ t('auth2.usernameOrMail') }}
                </label>
                <input
                    id="verify-username"
                    v-model="auth.username"
                    type="text"
                    name="username"
                    autocomplete="username"
                    autocapitalize="off"
                    spellcheck="false"
                    class="input input-lg w-full"
                    required
                />
            </div>

            <div>
                <label class="block text-sm font-semibold mb-1" for="verify-code">{{ t('auth2.codeLabel') }}</label>
                <input
                    id="verify-code"
                    ref="codeInput"
                    v-model="verificationCode"
                    type="text"
                    name="code"
                    inputmode="numeric"
                    autocomplete="one-time-code"
                    :placeholder="t('auth2.codePlaceholder')"
                    class="input input-lg w-full tracking-widest text-center text-2xl"
                    :class="formError && 'input-error'"
                    required
                />
                <p class="mt-1 text-sm opacity-70">{{ t('auth2.codeHelp') }}</p>
            </div>

            <div v-if="formError" role="alert" class="alert alert-error text-base">
                <i class="bi bi-exclamation-triangle-fill text-xl" />
                <span class="font-semibold">{{ formError }}</span>
            </div>

            <button type="submit" class="btn btn-primary btn-lg w-full" :disabled="submitting">
                <span v-if="submitting" class="loading loading-spinner" />
                {{ t('auth2.verifyButton') }}
            </button>
        </form>

        <template #footer>
            <RouterLink
                :to="{ name: 'login' }"
                class="link link-primary font-semibold"
                @click="auth.cancelVerification()"
            >
                <i class="bi bi-arrow-left" />
                {{ t('auth2.backToLogin') }}
            </RouterLink>
        </template>
    </AuthShell>
</template>
