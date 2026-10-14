<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import ConfirmModal from '@/components/ui/ConfirmModal.vue'
import StatusPill from '@/components/ui/StatusPill.vue'
import type { StatusKind } from '@/components/ui/StatusPill.vue'

import { authFetch } from '@/composables/authFetch'
import { formatDuration } from '@/composables/commercialApi'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'
import { useLive } from '@/stores/live'
import type { ChannelState } from '@/stores/live'

defineProps<{
    /** Title and start of the program after the current one, when known. */
    next?: { title: string; time: string } | null
}>()

type Action = 'start' | 'stop' | 'restart' | 'skip'

const { t } = useI18n()
const configStore = useConfig()
const indexStore = useIndex()
const live = useLive()

const asking = ref<Action | null>(null)
const busy = ref(false)

const kinds: Record<ChannelState, StatusKind> = {
    on_air: 'ok',
    live: 'live',
    restarting: 'warn',
    stopped: 'bad',
    offline: 'idle',
}

const running = computed(() => live.state === 'on_air' || live.state === 'live')
const title = computed(() => {
    const media = live.media

    if (!media) {
        return ''
    }

    return media.title || media.source.split('/').pop() || t('dash.now.filler')
})

async function run(action: Action) {
    const id = configStore.channels[configStore.i]?.id
    const call =
        action === 'skip'
            ? { path: 'playout', body: { control: 'next' } }
            : { path: 'process', body: { command: action } }

    busy.value = true

    try {
        const result = await authFetch<{ reason?: string }>(`/api/control/${id}/${call.path}`, {
            method: 'POST',
            headers: { 'content-type': 'application/json;charset=UTF-8' },
            body: JSON.stringify(call.body),
        })

        if (result?.reason === 'live_ingest_active') {
            indexStore.msgAlert('warning', t('dash.now.noSkipLive'), 4)
        } else {
            indexStore.msgAlert('success', t(`dash.now.done.${action}`), 3)
        }
    } catch (error) {
        indexStore.msgAlert('error', error instanceof Error ? error.message : String(error), 5)
    } finally {
        busy.value = false
    }
}

function ask(action: Action) {
    if (action === 'start') {
        void run(action)
    } else {
        asking.value = action
    }
}

function answer(confirmed: boolean) {
    const action = asking.value

    asking.value = null

    if (confirmed && action) {
        void run(action)
    }
}
</script>
<template>
    <section class="card bg-base-100 shadow p-5">
        <div class="flex flex-wrap items-center gap-3">
            <StatusPill :kind="kinds[live.state]" :label="t(`live.${live.state}`)" size="lg" />
            <p class="opacity-80 grow min-w-48">{{ t(`live.${live.state}_help`) }}</p>
        </div>

        <div v-if="running" class="mt-5">
            <div class="text-sm uppercase tracking-wide opacity-60">{{ t('dash.now.playing') }}</div>
            <div class="text-2xl font-bold truncate" :title="title">{{ title }}</div>
            <progress class="progress progress-primary w-full mt-2 h-3" :value="live.progress" max="100" />
            <div class="mt-1 flex flex-wrap justify-between gap-2 text-sm opacity-80">
                <span>{{ t('dash.now.left', { time: formatDuration(live.secondsLeft) }) }}</span>
                <span v-if="next">{{ t('dash.now.next', { title: next.title, time: next.time }) }}</span>
            </div>
            <div v-if="Math.abs(live.shift) > 2" class="mt-2 text-sm text-warning">
                <i class="bi bi-clock-history" /> {{ t('dash.now.shift', { seconds: Math.round(live.shift) }) }}
            </div>
        </div>

        <div class="mt-5 flex flex-wrap gap-2">
            <button v-if="!running" class="btn btn-primary btn-lg" :disabled="busy" @click="ask('start')">
                <i class="bi bi-play-fill text-2xl" /> {{ t('dash.now.start') }}
            </button>
            <button v-if="running && !live.ingest" class="btn btn-primary" :disabled="busy" @click="ask('skip')">
                <i class="bi bi-skip-forward-fill" /> {{ t('dash.now.skip') }}
            </button>
            <button v-if="running || live.state === 'restarting'" class="btn" :disabled="busy" @click="ask('restart')">
                <i class="bi bi-arrow-clockwise" /> {{ t('dash.now.restart') }}
            </button>
            <button
                v-if="running || live.state === 'restarting'"
                class="btn btn-error btn-outline"
                :disabled="busy"
                @click="ask('stop')"
            >
                <i class="bi bi-stop-fill" /> {{ t('dash.now.stop') }}
            </button>
        </div>

        <ConfirmModal
            :show="asking !== null"
            :danger="asking === 'stop' || asking === 'restart'"
            :title="asking ? t(`dash.now.confirm.${asking}.title`) : ''"
            :text="asking ? t(`dash.now.confirm.${asking}.text`) : ''"
            :confirm-label="asking ? t(`dash.now.confirm.${asking}.yes`) : ''"
            @confirm="answer(true)"
            @cancel="answer(false)"
        />
    </section>
</template>
