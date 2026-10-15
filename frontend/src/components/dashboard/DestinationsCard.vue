<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'

import GenericModal from '@/components/utils/GenericModal.vue'
import HelpTip from '@/components/ui/HelpTip.vue'

import { friendlyError } from '@/composables/friendlyError'
import { healthApi, type ChannelHealth, type RelayConfig } from '@/composables/healthApi'
import { useIndex } from '@/stores/index'

const props = defineProps<{ relay: ChannelHealth['relay']; channel: number; canManage: boolean }>()
const emit = defineEmits<{ (event: 'saved'): void }>()

const { t } = useI18n()
const indexStore = useIndex()

const editing = ref<RelayConfig | null>(null)
const saving = ref(false)

const lights = {
    connected: 'bg-success',
    disconnected: 'bg-error',
    unknown: 'bg-base-300',
} as const

async function configure() {
    try {
        editing.value = await healthApi.relay(props.channel)
    } catch (error) {
        indexStore.msgAlert('error', friendlyError(error), 6)
    }
}

function addDestination() {
    editing.value?.destinations.push({
        name: '',
        protocol: 'rtmp',
        match_user: '',
        match_address: '',
        enabled: true,
    })
}

async function save() {
    if (!editing.value) {
        return
    }

    saving.value = true

    try {
        await healthApi.saveRelay(props.channel, editing.value)
        editing.value = null
        indexStore.msgAlert('success', t('dash.dest.saved'), 3)
        emit('saved')
    } catch (error) {
        indexStore.msgAlert('error', friendlyError(error), 6)
    } finally {
        saving.value = false
    }
}
</script>
<template>
    <section class="card bg-base-100 shadow p-5">
        <div class="flex items-center gap-2">
            <h2 class="text-xl font-bold grow">{{ t('dash.dest.title') }} <HelpTip :text="t('dash.dest.help')" /></h2>
            <button v-if="canManage" class="btn btn-sm btn-ghost" @click="configure">
                <i class="bi bi-gear" /> {{ t('dash.dest.configure') }}
            </button>
        </div>

        <p v-if="!relay.configured" class="mt-2 opacity-70">
            {{ canManage ? t('dash.dest.notConfigured') : t('dash.dest.notConfiguredUser') }}
        </p>
        <template v-else>
            <p v-if="!relay.reachable" class="mt-2 text-warning font-semibold">
                <i class="bi bi-exclamation-triangle-fill me-1" />{{ t('dash.dest.unreachable') }}
            </p>
            <p v-else-if="!relay.destinations.length" class="mt-2 opacity-70">{{ t('dash.dest.none') }}</p>
            <ul class="mt-3 grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
                <li
                    v-for="destination in relay.destinations"
                    :key="destination.id"
                    class="flex items-center gap-3 rounded-lg bg-base-200 p-3"
                >
                    <span
                        class="inline-block size-4 shrink-0 rounded-full"
                        :class="lights[destination.state]"
                        role="img"
                        :aria-label="t(`dash.dest.state.${destination.state}`)"
                    />
                    <span class="grow min-w-0">
                        <span class="block font-semibold truncate">{{ destination.name }}</span>
                        <span class="block text-xs opacity-70">
                            {{ destination.protocol.toUpperCase() }} · {{ t(`dash.dest.state.${destination.state}`) }}
                        </span>
                    </span>
                </li>
            </ul>
            <p v-if="relay.others > 0" class="mt-2 text-sm opacity-70">
                {{ t('dash.dest.others', { count: relay.others }) }}
            </p>
        </template>

        <GenericModal
            :show="editing !== null"
            :title="t('dash.dest.configure')"
            :hide-buttons="true"
            :modal-action="() => (editing = null)"
        >
            <form v-if="editing" class="w-180 max-w-full space-y-3" @submit.prevent="save">
                <p class="text-sm opacity-70">{{ t('dash.dest.configHelp') }}</p>
                <div class="grid gap-3 sm:grid-cols-2">
                    <label class="form-control">
                        <span class="label-text mb-1">{{ t('dash.dest.apiUrl') }}</span>
                        <input
                            v-model="editing.api_url"
                            class="input input-sm w-full"
                            placeholder="http://127.0.0.1:9997"
                        />
                    </label>
                    <label class="form-control">
                        <span class="label-text mb-1">{{ t('dash.dest.path') }}</span>
                        <input v-model="editing.path" class="input input-sm w-full" placeholder="canal1" />
                    </label>
                </div>

                <div class="space-y-2">
                    <div
                        v-for="(destination, index) in editing.destinations"
                        :key="index"
                        class="grid gap-2 sm:grid-cols-[1fr_6rem_1fr_1fr_auto_auto] items-end rounded-lg border border-base-300 p-2"
                    >
                        <label class="form-control">
                            <span class="text-xs opacity-70">{{ t('dash.dest.name') }}</span>
                            <input v-model="destination.name" class="input input-sm w-full" required />
                        </label>
                        <label class="form-control">
                            <span class="text-xs opacity-70">{{ t('dash.dest.protocol') }}</span>
                            <select v-model="destination.protocol" class="select select-sm w-full">
                                <option value="rtmp">RTMP</option>
                                <option value="srt">SRT</option>
                            </select>
                        </label>
                        <label class="form-control">
                            <span class="text-xs opacity-70">{{ t('dash.dest.user') }}</span>
                            <input v-model="destination.match_user" class="input input-sm w-full" />
                        </label>
                        <label class="form-control">
                            <span class="text-xs opacity-70">{{ t('dash.dest.address') }}</span>
                            <input v-model="destination.match_address" class="input input-sm w-full" />
                        </label>
                        <input
                            v-model="destination.enabled"
                            type="checkbox"
                            class="toggle toggle-sm toggle-accent mb-2"
                            :title="t('dash.dest.enabled')"
                            :aria-label="t('dash.dest.enabled')"
                        />
                        <button
                            type="button"
                            class="btn btn-sm btn-ghost"
                            :aria-label="t('dash.dest.remove')"
                            @click="editing.destinations.splice(index, 1)"
                        >
                            <i class="bi bi-trash" />
                        </button>
                    </div>
                </div>
                <p class="text-xs opacity-70">{{ t('dash.dest.matchHelp') }}</p>

                <div class="flex justify-between gap-2">
                    <button type="button" class="btn btn-sm" @click="addDestination">
                        <i class="bi bi-plus-lg" /> {{ t('dash.dest.add') }}
                    </button>
                    <span class="flex gap-2">
                        <button type="button" class="btn btn-sm" @click="editing = null">{{ t('cancel') }}</button>
                        <button type="submit" class="btn btn-sm btn-accent" :disabled="saving">
                            {{ t('dash.dest.save') }}
                        </button>
                    </span>
                </div>
            </form>
        </GenericModal>
    </section>
</template>
