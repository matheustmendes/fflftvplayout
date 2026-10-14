<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useEventSource } from '@vueuse/core'
import { useI18n } from 'vue-i18n'
import { cloneDeep } from 'es-toolkit/object'

import AdvancedBlock from '@/components/ui/AdvancedBlock.vue'
import SaveBar from '@/components/ui/SaveBar.vue'
import DiscardModal from '@/components/config/DiscardModal.vue'
import RestartModal from '@/components/config/RestartModal.vue'
import SettingsCard from '@/components/config/SettingsCard.vue'
import SettingsField from '@/components/config/SettingsField.vue'
import SettingsGroup from '@/components/config/SettingsGroup.vue'

import { usePlayoutDirty } from '@/composables/settingsHelpers'
import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'
import { authFetch } from '@/composables/authFetch'

const { t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const dirty = usePlayoutDirty()
const saving = ref(false)
const showDiscardModal = ref(false)
const audioLevel = ref<AudioLevel | null>(null)
const loudness = ref<LiveLoudnessMetrics | null>(null)
const streamUrl = ref(
    `/data/event/${configStore.channels[configStore.i]?.id}?endpoint=playout&audio_meter=true&uuid=${authStore.uuid}`,
)

const { data, close } = useEventSource(streamUrl, [], {
    autoReconnect: { retries: -1, delay: 1000 },
})

type AudioConfig = PlayoutConfigExt['audio']
type AudioNumberKey = Exclude<keyof AudioConfig, 'loudness_scope' | 'loudness_enable'>

interface AudioField {
    key: AudioNumberKey
    min?: number
    max?: number
    step: number
    // Shown as "Padrão: ..." under the field.
    defaultText: string
}

// Technical leveling numbers; the simple view only offers volume and the on/off choice.
const levelingFields: AudioField[] = [
    { key: 'compressor_ratio', min: 1, max: 10, step: 0.5, defaultText: '3:1' },
    { key: 'pause_threshold_dbfs', min: -90, max: -20, step: 1, defaultText: '−55 dBFS' },
    { key: 'loudness_target_lufs', step: 0.1, defaultText: '−23 LUFS' },
    { key: 'loudness_true_peak_ceiling_dbtp', max: 0, step: 0.1, defaultText: '−1 dBTP' },
    { key: 'loudness_max_gain_db', min: 0, step: 0.1, defaultText: '+8 dB' },
    { key: 'loudness_max_attenuation_db', max: 0, step: 0.1, defaultText: '−12 dB' },
    { key: 'compressor_attack_ms', min: 0.1, max: 50, step: 0.1, defaultText: '5 ms' },
    { key: 'compressor_hold_ms', min: 0, max: 2000, step: 0.1, defaultText: '100 ms' },
    { key: 'compressor_release_ms', min: 10, max: 10000, step: 0.1, defaultText: '1200 ms' },
    { key: 'compressor_strong_release_ms', min: 10, max: 10000, step: 0.1, defaultText: '500 ms' },
    { key: 'compressor_knee_db', min: 0, max: 24, step: 0.1, defaultText: '6 dB' },
    { key: 'pause_hold_ms', min: 0, max: 5000, step: 0.1, defaultText: '300 ms' },
    { key: 'pause_return_delay_ms', min: 0, max: 30000, step: 0.1, defaultText: '2000 ms' },
    { key: 'loudness_output_max_correction_db', min: 0, max: 12, step: 0.1, defaultText: '±3 dB' },
    { key: 'loudness_output_gain_up_db_per_second', min: 0, max: 5, step: 0.1, defaultText: '0.1 dB/s' },
    { key: 'loudness_output_gain_down_db_per_second', min: 0, max: 5, step: 0.05, defaultText: '0.25 dB/s' },
    { key: 'compressor_threshold_dbfs', min: -60, max: 0, step: 1, defaultText: '−26 dBFS' },
    { key: 'loudness_dead_band_lu', min: 0, step: 0.1, defaultText: '1 LU' },
    { key: 'loudness_silence_gate_lufs', step: 0.1, defaultText: '−60 LUFS' },
    { key: 'loudness_gain_up_db_per_second', min: 0, step: 0.1, defaultText: '0.5 dB/s' },
    { key: 'loudness_gain_down_db_per_second', min: 0, step: 0.1, defaultText: '2 dB/s' },
]

const volumePercent = computed(() => Math.round((configStore.playout.audio.volume ?? 0) * 100))

watch(data, () => {
    if (!data.value) return

    try {
        const status = JSON.parse(data.value) as PlayoutStatus
        audioLevel.value = status.audio ?? null
        loudness.value = status.loudness ?? null
    } catch {
        // The connection banner and keep-alive messages are not status JSON.
    }
})

watch(
    () => configStore.i,
    () => {
        audioLevel.value = null
        loudness.value = null
        streamUrl.value = `/data/event/${configStore.channels[configStore.i]?.id}?endpoint=playout&audio_meter=true&uuid=${authStore.uuid}`
    },
)

onBeforeUnmount(close)

function meterPercent(value: number | null | undefined, floor = -60) {
    if (value == null) return 0
    return Math.min(100, Math.max(0, ((value - floor) / -floor) * 100))
}

function meterValue(value: number | null | undefined, unit: string) {
    return value == null ? '—' : `${value.toFixed(1)} ${unit}`
}

function discardChanges() {
    configStore.playout = cloneDeep(configStore.playoutSaved)
    showDiscardModal.value = false
}

async function saveAudio() {
    if (saving.value) {
        return
    }

    saving.value = true

    try {
        const result = await configStore.setPlayoutConfig(configStore.playout)
        if (result.requires_restart) {
            const id = configStore.channels[configStore.i]?.id
            const status = await authFetch<string>(`/api/control/${id}/process`, {
                method: 'POST',
                headers: { ...configStore.contentType, ...authStore.authHeader },
                body: JSON.stringify({ command: 'status' }),
            })
            if (status === 'active') configStore.showRestartModal = true
        }
    } catch (error) {
        const detail = error instanceof Error ? error.message : String(error)
        indexStore.msgAlert('error', `${t('settings2.audio.saveFailed')} ${detail}`, 4)
        saving.value = false
        return
    }

    indexStore.msgAlert('success', t('settings2.audio.saved'), 2)
    await configStore.getPlayoutConfig()
    saving.value = false
}
</script>

<template>
    <form v-if="configStore.playout" class="space-y-5" @submit.prevent="saveAudio">
        <SettingsCard
            icon="bi-volume-up"
            :title="t('settings2.audio.volumeTitle')"
            :text="t('settings2.audio.volumeText')"
        >
            <SettingsField
                :label="t('settings2.audio.volume')"
                :help="t('settings2.audio.volumeHelp')"
                :hint="t('settings2.audio.volumeDefault')"
            >
                <div class="flex items-center gap-4">
                    <i class="bi bi-volume-down text-xl opacity-70" />
                    <input
                        v-model.number="configStore.playout.audio.volume"
                        type="range"
                        min="0"
                        max="1.5"
                        step="0.01"
                        class="range range-primary grow"
                        :aria-label="t('settings2.audio.volume')"
                    />
                    <i class="bi bi-volume-up text-xl opacity-70" />
                    <span class="w-16 text-end text-xl font-bold tabular-nums">{{ volumePercent }}%</span>
                </div>
            </SettingsField>
        </SettingsCard>

        <SettingsCard
            icon="bi-soundwave"
            :title="t('settings2.audio.levelingTitle')"
            :text="t('settings2.audio.levelingText')"
        >
            <SettingsField
                :label="t('settings2.audio.scope')"
                :help="t('settings2.audio.scopeHelp')"
                :hint="t('settings2.audio.scopeNote')"
            >
                <select v-model="configStore.playout.audio.loudness_scope" class="select w-full max-w-md">
                    <option value="all">{{ t('settings2.audio.scopeAll') }}</option>
                    <option value="live">{{ t('settings2.audio.scopeLive') }}</option>
                    <option value="off">{{ t('settings2.audio.scopeOff') }}</option>
                </select>
            </SettingsField>
        </SettingsCard>

        <SettingsCard
            icon="bi-speedometer2"
            :title="t('settings2.audio.metersTitle')"
            :text="t('settings2.audio.metersText')"
        >
            <section class="grid gap-4 sm:grid-cols-2" aria-label="Program output audio meters">
                <div class="rounded-box border border-base-300 bg-base-200 p-4">
                    <SettingsField :label="t('settings2.audio.peak')" :help="t('settings2.audio.peakHelp')">
                        <span class="font-mono text-sm">{{ meterValue(audioLevel?.peak_db, 'dBFS') }}</span>
                    </SettingsField>
                    <div class="mt-3 h-3 overflow-hidden rounded-full bg-base-300">
                        <div
                            class="h-full bg-success transition-[width] duration-300"
                            :style="{ width: `${meterPercent(audioLevel?.peak_db)}%` }"
                        />
                    </div>
                    <p class="mt-2 text-sm opacity-70">
                        {{ t('settings2.audio.average', { value: meterValue(audioLevel?.rms_db, 'dBFS') }) }}
                    </p>
                </div>
                <div class="rounded-box border border-base-300 bg-base-200 p-4">
                    <SettingsField :label="t('settings2.audio.loudness')" :help="t('settings2.audio.loudnessHelp')">
                        <span class="font-mono text-sm">{{ meterValue(loudness?.short_term_lufs, 'LUFS') }}</span>
                    </SettingsField>
                    <div class="mt-3 h-3 overflow-hidden rounded-full bg-base-300">
                        <div
                            class="h-full bg-success transition-[width] duration-300"
                            :style="{ width: `${meterPercent(loudness?.short_term_lufs)}%` }"
                        />
                    </div>
                    <p class="mt-2 text-sm opacity-70">
                        {{ t('settings2.audio.integrated', { value: meterValue(loudness?.integrated_lufs, 'LUFS') }) }}
                        ·
                        {{ t('settings2.audio.truePeak', { value: meterValue(loudness?.true_peak_dbtp, 'dBTP') }) }}
                    </p>
                </div>
            </section>
        </SettingsCard>

        <AdvancedBlock v-if="configStore.playout.audio.loudness_scope !== 'off'">
            <SettingsGroup :title="t('settings2.audio.advancedTitle')" :text="t('settings2.audio.advancedText')">
                <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
                    <SettingsField
                        v-for="field in levelingFields"
                        :key="field.key"
                        :label="t(`settings2.audio.fields.${field.key}`)"
                        :hint="t('config.audioDefaultValue', { value: field.defaultText })"
                    >
                        <input
                            v-model.number="configStore.playout.audio[field.key]"
                            type="number"
                            :min="field.min"
                            :max="field.max"
                            :step="field.step"
                            class="input input-sm w-full"
                        />
                    </SettingsField>
                </div>
            </SettingsGroup>
        </AdvancedBlock>

        <SaveBar :dirty="dirty" :saving="saving" @discard="showDiscardModal = true" />
    </form>

    <DiscardModal :show="showDiscardModal" @confirm="discardChanges" @cancel="showDiscardModal = false" />
    <RestartModal />
</template>
