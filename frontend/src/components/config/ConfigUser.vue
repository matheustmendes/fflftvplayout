<script setup lang="ts">
import { computed, ref, onMounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { cloneDeep } from 'es-toolkit/object'
import { isEqual } from 'es-toolkit/predicate'

import Multiselect from '@vueform/multiselect'

import ConfirmModal from '@/components/ui/ConfirmModal.vue'
import SaveBar from '@/components/ui/SaveBar.vue'
import SettingsCard from '@/components/config/SettingsCard.vue'
import SettingsField from '@/components/config/SettingsField.vue'

import { authFetch } from '@/composables/authFetch'
import { useVariables } from '@/composables/variables'
import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'
import { useConfig } from '@/stores/config'

const { t } = useI18n()
const { multiSelectClasses } = useVariables()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const selected = ref(null as null | number)
const users = ref([] as User[])
const showUserModal = ref(false)
const showDeleteModal = ref(false)
const saving = ref(false)
const newPass = ref('')
const confirmPass = ref('')
const savedUser = ref({} as User)

const user = ref({
    id: 0,
    username: '',
    mail: '',
    password: '',
    confirm: '',
    admin: false,
    channel_ids: [configStore.channels[configStore.i]?.id ?? 1],
    role_id: 3,
    two_factor: true,
} as User)

const isGlobalAdmin = computed(() => authStore.role === 'global_admin')
const isEditingSelf = computed(() => configStore.configUser.id === configStore.currentUser)
const dirty = computed(
    () => !isEqual(configStore.configUser, savedUser.value) || newPass.value.length > 0 || confirmPass.value.length > 0,
)
const roleLabel = computed(() => {
    const roleId = configStore.configUser.role_id

    if (roleId === 1) return t('settings2.account.roleGlobal')
    if (roleId === 2) return t('settings2.account.roleAdmin')

    return t('settings2.account.roleUser')
})

// Remember what the server has, so the page can tell what was changed and undo it.
watch(
    () => configStore.configUser,
    (current) => {
        savedUser.value = cloneDeep(current)
    },
    { immediate: true },
)

onMounted(() => {
    if (isGlobalAdmin.value) {
        getUsers()
    }
})

async function getUsers() {
    const data = await authFetch<User[]>('/api/users', {
        method: 'GET',
        headers: authStore.authHeader,
    })

    users.value = data
    selected.value = configStore.currentUser
}

function onChange(event: Event) {
    const target = event.target as HTMLSelectElement
    selected.value = Number(target.value)
    newPass.value = ''
    confirmPass.value = ''

    getUserConfig()
}

async function getUserConfig() {
    await authFetch<User>(`/api/user/${selected.value}`, {
        method: 'GET',
        headers: authStore.authHeader,
    }).then((data) => {
        configStore.configUser = data
    })
}

function askDeleteUser() {
    if (isEditingSelf.value) {
        indexStore.msgAlert('error', t('settings2.account.deleteSelf'), 3)
        return
    }

    showDeleteModal.value = true
}

async function deleteUser() {
    showDeleteModal.value = false

    await authFetch(`/api/user/${configStore.configUser.id}`, {
        method: 'DELETE',
        headers: authStore.authHeader,
    })
        .then(async () => {
            indexStore.msgAlert('success', t('settings2.account.deleted'), 2)

            await configStore.getUserConfig()
            await getUsers()
        })
        .catch(() => {
            indexStore.msgAlert('error', t('settings2.account.deleteFailed'), 3)
        })
}

function clearUser() {
    user.value.id = 0
    user.value.username = ''
    user.value.mail = ''
    user.value.password = ''
    user.value.confirm = ''
    user.value.admin = false
    user.value.channel_ids = [1]
    user.value.role_id = 3
    user.value.two_factor = true
}

function closeUserModal() {
    showUserModal.value = false
    clearUser()
}

async function addUser() {
    if (!user.value.username || !user.value.password || user.value.password !== user.value.confirm) {
        indexStore.msgAlert('error', t('settings2.account.mismatch'), 3)
        return
    }

    // The form keeps its own "admin" checkbox; the server only knows the role id.
    const payload: User = { ...user.value, role_id: user.value.admin ? 2 : 3 }
    delete payload.admin

    await authStore.inspectToken()

    try {
        const username = user.value.username
        await configStore.addNewUser(payload)
        showUserModal.value = false

        indexStore.msgAlert('success', t('settings2.account.added'), 2)
        await getUsers()
        const createdUser = users.value.find((item) => item.username === username)
        if (createdUser) {
            selected.value = createdUser.id
            await getUserConfig()
        }

        clearUser()
    } catch {
        indexStore.msgAlert('error', t('settings2.account.addFailed'), 4)
    }
}

function discardChanges() {
    configStore.configUser = cloneDeep(savedUser.value)
    newPass.value = ''
    confirmPass.value = ''
}

async function onSubmitUser() {
    if (saving.value) {
        return
    }

    if (newPass.value) {
        if (newPass.value === confirmPass.value) {
            configStore.configUser.password = newPass.value
        } else {
            indexStore.msgAlert('error', t('settings2.account.mismatch'), 4)
            return
        }
    }

    saving.value = true
    await authStore.inspectToken()
    try {
        await configStore.setUserConfig(configStore.configUser)
        savedUser.value = cloneDeep(configStore.configUser)
        newPass.value = ''
        confirmPass.value = ''
        indexStore.msgAlert('success', t('settings2.account.saved'), 2)
    } catch {
        indexStore.msgAlert('error', t('settings2.account.saveFailed'), 4)
    } finally {
        saving.value = false
    }
}
</script>
<template>
    <div class="space-y-5">
        <SettingsCard
            v-if="isGlobalAdmin"
            icon="bi-people"
            :title="t('settings2.account.picker')"
            :text="t('settings2.account.pickerHelp')"
        >
            <div class="flex flex-wrap items-center gap-2">
                <select
                    v-model="selected"
                    class="select grow min-w-48 max-w-md"
                    :aria-label="t('settings2.account.picker')"
                    @change="onChange($event)"
                >
                    <option v-for="item in users" :key="item.username" :value="item.id">{{ item.username }}</option>
                </select>
                <button type="button" class="btn btn-primary" @click="showUserModal = true">
                    <i class="bi bi-person-plus" />
                    {{ t('settings2.account.add') }}
                </button>
                <button type="button" class="btn btn-error btn-outline" @click="askDeleteUser">
                    <i class="bi bi-person-x" />
                    {{ t('settings2.account.delete') }}
                </button>
            </div>
        </SettingsCard>

        <form v-if="configStore.configUser" class="space-y-5" @submit.prevent="onSubmitUser">
            <SettingsCard
                icon="bi-person-circle"
                :title="
                    isGlobalAdmin && !isEditingSelf ? t('settings2.account.titleOther') : t('settings2.account.title')
                "
                :text="t('settings2.account.text')"
            >
                <template #actions>
                    <span class="badge badge-outline">{{ roleLabel }}</span>
                </template>
                <SettingsField :label="t('settings2.account.username')" :hint="t('settings2.account.usernameHint')">
                    <input
                        v-model="configStore.configUser.username"
                        type="text"
                        name="username"
                        class="input w-full bg-base-200!"
                        disabled
                    />
                </SettingsField>
                <SettingsField :label="t('settings2.account.mail')" :help="t('settings2.account.mailHelp')">
                    <input v-model="configStore.configUser.mail" type="email" name="mail" class="input w-full" />
                </SettingsField>
            </SettingsCard>

            <SettingsCard
                icon="bi-key"
                :title="t('settings2.account.passwordTitle')"
                :text="t('settings2.account.passwordText')"
            >
                <div class="grid gap-4 sm:grid-cols-2">
                    <SettingsField :label="t('settings2.account.newPass')">
                        <input
                            v-model="newPass"
                            type="password"
                            name="password"
                            autocomplete="new-password"
                            class="input w-full"
                        />
                    </SettingsField>
                    <SettingsField :label="t('settings2.account.confirmPass')">
                        <input
                            v-model="confirmPass"
                            type="password"
                            name="password_confirm"
                            autocomplete="new-password"
                            class="input w-full"
                        />
                    </SettingsField>
                </div>
            </SettingsCard>

            <SettingsCard
                v-if="isGlobalAdmin"
                icon="bi-shield-check"
                :title="t('settings2.account.accessTitle')"
                :text="t('settings2.account.accessText')"
            >
                <label class="flex cursor-pointer items-center gap-3">
                    <input v-model="configStore.configUser.two_factor" type="checkbox" class="toggle toggle-primary" />
                    <span class="font-semibold">{{ t('settings2.account.twoFactor') }}</span>
                </label>
                <p class="text-sm opacity-70">{{ t('settings2.account.twoFactorHelp') }}</p>

                <SettingsField :label="t('settings2.account.channels')" :help="t('settings2.account.channelsHelp')">
                    <Multiselect
                        v-model="configStore.configUser.channel_ids"
                        :options="configStore.channels"
                        mode="tags"
                        :close-on-select="true"
                        :can-clear="false"
                        label="name"
                        value-prop="id"
                        :classes="multiSelectClasses"
                        :disabled="configStore.configUser.role_id === 1"
                    />
                </SettingsField>
            </SettingsCard>

            <SaveBar :dirty="dirty" :saving="saving" require-change @discard="discardChanges" />
        </form>
    </div>

    <div
        v-if="showUserModal"
        class="fixed inset-0 z-50 flex items-start justify-center overflow-auto bg-black/40 p-4 sm:items-center"
        role="dialog"
        aria-modal="true"
        @click.self="closeUserModal"
        @keydown.esc="closeUserModal"
    >
        <form class="w-full max-w-lg rounded-2xl bg-base-100 p-6 shadow-2xl" @submit.prevent="addUser">
            <h2 class="text-xl font-bold">{{ t('settings2.account.addTitle') }}</h2>
            <p class="mt-1 text-sm opacity-70">{{ t('settings2.account.addText') }}</p>

            <div class="mt-4 space-y-4">
                <SettingsField :label="t('settings2.account.username')">
                    <input v-model="user.username" type="text" name="new_username" class="input w-full" />
                </SettingsField>
                <SettingsField :label="t('settings2.account.mail')">
                    <input v-model="user.mail" type="email" name="new_mail" class="input w-full" />
                </SettingsField>
                <div class="grid gap-4 sm:grid-cols-2">
                    <SettingsField :label="t('settings2.account.password')">
                        <input
                            v-model="user.password"
                            type="password"
                            name="new_password"
                            autocomplete="new-password"
                            class="input w-full"
                        />
                    </SettingsField>
                    <SettingsField :label="t('settings2.account.confirmPass')">
                        <input
                            v-model="user.confirm"
                            type="password"
                            name="new_password_confirm"
                            autocomplete="new-password"
                            class="input w-full"
                        />
                    </SettingsField>
                </div>
                <SettingsField :label="t('settings2.account.channels')" :help="t('settings2.account.channelsHelp')">
                    <Multiselect
                        v-model="user.channel_ids"
                        :options="configStore.channels"
                        mode="tags"
                        :close-on-select="true"
                        :can-clear="false"
                        label="name"
                        value-prop="id"
                        :classes="multiSelectClasses"
                    />
                </SettingsField>
                <label class="flex cursor-pointer items-center gap-3">
                    <input v-model="user.admin" type="checkbox" class="toggle toggle-primary" />
                    <span class="font-semibold">{{ t('settings2.account.admin') }}</span>
                </label>
                <p class="text-sm opacity-70">{{ t('settings2.account.adminHelp') }}</p>
                <label class="flex cursor-pointer items-center gap-3">
                    <input v-model="user.two_factor" type="checkbox" class="toggle toggle-primary" />
                    <span class="font-semibold">{{ t('settings2.account.twoFactor') }}</span>
                </label>
            </div>

            <div class="mt-6 flex flex-wrap justify-end gap-2">
                <button type="button" class="btn" @click="closeUserModal">{{ t('ui.cancel') }}</button>
                <button type="submit" class="btn btn-primary">{{ t('settings2.account.create') }}</button>
            </div>
        </form>
    </div>

    <ConfirmModal
        :show="showDeleteModal"
        danger
        :title="t('settings2.account.deleteConfirmTitle', { name: configStore.configUser.username })"
        :text="t('settings2.account.deleteConfirmText')"
        :confirm-label="t('settings2.account.deleteConfirm')"
        @confirm="deleteUser"
        @cancel="showDeleteModal = false"
    />
</template>
