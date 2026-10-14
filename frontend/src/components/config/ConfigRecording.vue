<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { cloneDeep } from 'es-toolkit/object'

import AdvancedBlock from '@/components/ui/AdvancedBlock.vue'
import SaveBar from '@/components/ui/SaveBar.vue'
import DiscardModal from '@/components/config/DiscardModal.vue'
import KeyValueEditor from '@/components/config/KeyValueEditor.vue'
import RestartModal from '@/components/config/RestartModal.vue'
import SettingsCard from '@/components/config/SettingsCard.vue'
import SettingsField from '@/components/config/SettingsField.vue'
import SettingsGroup from '@/components/config/SettingsGroup.vue'

import { authFetch } from '@/composables/authFetch'
import { usePlayoutDirty } from '@/composables/settingsHelpers'
import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'

const { t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const dirty = usePlayoutDirty()

const saving = ref(false)
const showDiscardModal = ref(false)

const activeOutput = computed(() => configStore.outputs.find((output) => output.id === configStore.playout.output.id))

const hlsVariants = computed(() => {
    const output = configStore.playout.output
    if (output.mode !== 'hls' || output.id !== configStore.playout.recording.source_output_id) return []

    return [
        output.hls_playlist_name || 'stream',
        ...output.hls_variants.map((variant) => variant.split(':')[0]).filter(Boolean),
    ]
})

const copyOutputs = computed(() => {
    const mode = configStore.playout.recording.source === 'hls_variant' ? 'hls' : 'stream'
    return activeOutput.value?.name === mode ? [activeOutput.value] : []
})

const videoCodecs = computed(() => configStore.outputCodecs.recording.video)
const audioCodecs = computed(() => configStore.outputCodecs.recording.audio)
const videoSettings = computed(
    () => videoCodecs.value.find((codec) => codec.name === configStore.playout.recording.video_codec)?.settings ?? [],
)
const audioUsesBitrate = computed(
    () =>
        audioCodecs.value.find((codec) => codec.name === configStore.playout.recording.audio_codec)?.uses_bitrate ??
        true,
)
function setRecordingVideoCodec() {
    const codec = videoCodecs.value.find((codec) => codec.name === configStore.playout.recording.video_codec)
    if (!codec) return
    configStore.playout.recording.video_options = Object.fromEntries(
        codec.settings.map((setting) => [setting.key, setting.default]),
    )
}

function setRecordingAudioCodec() {
    configStore.playout.recording.audio_options = {}
}

function setRecordingSource() {
    const recording = configStore.playout.recording
    if (recording.source === 'encode') return

    recording.source_output_id = activeOutput.value?.id ?? null
    if (recording.source === 'hls_variant') {
        recording.variant = hlsVariants.value[0] ?? ''
    }
}

function settingIsVisible(setting: EncoderSetting) {
    const condition = setting.visible_when
    return !condition || configStore.playout.recording.video_options[condition.key] === condition.value
}

function setVideoOption(key: string, value: string | number) {
    configStore.playout.recording.video_options = {
        ...configStore.playout.recording.video_options,
        [key]: String(value),
    }
}

function eventValue(event: Event) {
    return (event.target as HTMLInputElement).value
}

function discardChanges() {
    configStore.playout = cloneDeep(configStore.playoutSaved)
    showDiscardModal.value = false
}

async function saveRecording() {
    if (saving.value) {
        return
    }

    saving.value = true

    try {
        const result = await configStore.setPlayoutConfig(configStore.playout)

        const id = configStore.channels[configStore.i]?.id
        const status = await authFetch<string>(`/api/control/${id}/process`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({ command: 'status' }),
        })
        if (status === 'active' && result.requires_restart) {
            configStore.showRestartModal = true
        }

        await configStore.getPlayoutConfig()
        indexStore.msgAlert('success', t('settings2.recording.saved'), 2)
    } catch (error) {
        const detail = error instanceof Error ? error.message : String(error)
        indexStore.msgAlert('error', `${t('settings2.recording.saveFailed')} ${detail}`, 4)
    } finally {
        saving.value = false
    }
}
</script>

<template>
    <form v-if="configStore.playout.recording" class="space-y-5" @submit.prevent="saveRecording">
        <SettingsCard
            icon="bi-record-circle"
            :title="t('settings2.recording.title')"
            :text="t('settings2.recording.text')"
        >
            <label class="flex cursor-pointer items-center gap-3">
                <input v-model="configStore.playout.recording.enable" type="checkbox" class="toggle toggle-primary" />
                <span class="font-semibold">{{ t('settings2.recording.enable') }}</span>
                <span class="badge" :class="configStore.playout.recording.enable ? 'badge-success' : 'badge-ghost'">
                    {{ configStore.playout.recording.enable ? t('settings2.on') : t('settings2.off') }}
                </span>
            </label>
            <p class="text-sm opacity-70">{{ t('settings2.recording.enableHelp') }}</p>

            <div class="grid gap-4 sm:grid-cols-2">
                <SettingsField
                    class="sm:col-span-2"
                    :label="t('settings2.recording.where')"
                    :help="t('settings2.recording.whereHelp')"
                >
                    <input
                        v-model.trim="configStore.playout.recording.path"
                        type="text"
                        class="input w-full"
                        :placeholder="t('settings2.recording.wherePlaceholder')"
                    />
                </SettingsField>
                <SettingsField
                    :label="t('settings2.recording.keep')"
                    :help="t('settings2.recording.keepHelp')"
                    :hint="t('settings2.recording.keepHint')"
                >
                    <input
                        v-model.number="configStore.playout.recording.retention_days"
                        type="number"
                        min="0"
                        step="1"
                        class="input w-full"
                    />
                </SettingsField>
            </div>
        </SettingsCard>

        <AdvancedBlock>
            <SettingsGroup :title="t('settings2.recording.advancedSource')">
                <div class="grid gap-3 sm:grid-cols-2">
                    <SettingsField :label="t('config.recordingSource')">
                        <select
                            v-model="configStore.playout.recording.source"
                            class="select select-sm w-full"
                            @change="setRecordingSource"
                        >
                            <option value="stream" :disabled="configStore.playout.output.mode !== 'stream'">
                                {{ t('config.recordingCurrentStream') }}
                            </option>
                            <option value="hls_variant" :disabled="configStore.playout.output.mode !== 'hls'">
                                {{ t('config.recordingHlsVariant') }}
                            </option>
                            <option value="encode">{{ t('config.recordingEncode') }}</option>
                        </select>
                    </SettingsField>
                    <SettingsField
                        v-if="configStore.playout.recording.source !== 'encode'"
                        :label="t('settings2.recording.sourceOutput')"
                    >
                        <select
                            v-model.number="configStore.playout.recording.source_output_id"
                            class="select select-sm w-full"
                        >
                            <option :value="null" disabled>{{ t('config.recordingSelectOutput') }}</option>
                            <option v-for="output in copyOutputs" :key="output.id" :value="output.id">
                                {{ output.name }}
                            </option>
                        </select>
                    </SettingsField>
                    <SettingsField
                        v-if="configStore.playout.recording.source === 'hls_variant'"
                        :label="t('settings2.recording.hlsVariant')"
                    >
                        <select v-model="configStore.playout.recording.variant" class="select select-sm w-full">
                            <option v-for="variant in hlsVariants" :key="variant" :value="variant">
                                {{ variant }}
                            </option>
                        </select>
                    </SettingsField>
                    <template v-if="configStore.playout.recording.source === 'encode'">
                        <SettingsField :label="t('config.videoCodec')">
                            <select
                                v-model="configStore.playout.recording.video_codec"
                                class="select select-sm w-full"
                                @change="setRecordingVideoCodec"
                            >
                                <option v-for="codec in videoCodecs" :key="codec.name" :value="codec.name">
                                    {{ codec.display_name }}
                                </option>
                            </select>
                        </SettingsField>
                        <SettingsField :label="t('config.audioCodec')">
                            <select
                                v-model="configStore.playout.recording.audio_codec"
                                class="select select-sm w-full"
                                @change="setRecordingAudioCodec"
                            >
                                <option v-for="codec in audioCodecs" :key="codec.name" :value="codec.name">
                                    {{ codec.display_name }}
                                </option>
                            </select>
                        </SettingsField>
                        <template v-for="setting in videoSettings" :key="setting.key">
                            <SettingsField v-if="settingIsVisible(setting)" :label="setting.label">
                                <select
                                    v-if="setting.kind === 'select'"
                                    :value="configStore.playout.recording.video_options[setting.key]"
                                    class="select select-sm w-full"
                                    @change="setVideoOption(setting.key, eventValue($event))"
                                >
                                    <option v-for="choice in setting.choices" :key="choice.value" :value="choice.value">
                                        {{ choice.label }}
                                    </option>
                                </select>
                                <input
                                    v-else
                                    :value="configStore.playout.recording.video_options[setting.key]"
                                    type="number"
                                    :min="setting.minimum ?? undefined"
                                    :max="setting.maximum ?? undefined"
                                    step="1"
                                    class="input input-sm w-full"
                                    @input="setVideoOption(setting.key, eventValue($event))"
                                />
                            </SettingsField>
                        </template>
                        <SettingsField :label="t('settings2.recording.width')">
                            <input
                                v-model.number="configStore.playout.recording.width"
                                type="number"
                                min="0"
                                step="2"
                                class="input input-sm w-full"
                            />
                        </SettingsField>
                        <SettingsField :label="t('settings2.recording.height')">
                            <input
                                v-model.number="configStore.playout.recording.height"
                                type="number"
                                min="0"
                                step="2"
                                class="input input-sm w-full"
                            />
                        </SettingsField>
                        <SettingsField v-if="audioUsesBitrate" :label="t('config.audioBitrate')">
                            <input
                                v-model.number="configStore.playout.recording.audio_bitrate"
                                type="number"
                                min="1"
                                step="1"
                                class="input input-sm w-full"
                            />
                        </SettingsField>
                    </template>
                </div>
                <SettingsField
                    v-if="configStore.playout.recording.source === 'encode'"
                    :label="t('config.audioEncoderOptions')"
                    :hint="t('config.audioEncoderOptionsHelp')"
                >
                    <KeyValueEditor
                        v-model="configStore.playout.recording.audio_options"
                        :name-placeholder="t('config.audioOptionName')"
                        :value-placeholder="t('config.audioOptionValue')"
                        :add-label="t('config.addAudioOption')"
                    />
                </SettingsField>
            </SettingsGroup>

            <SettingsGroup :title="t('settings2.recording.advancedFiles')">
                <div class="grid gap-3 sm:grid-cols-2">
                    <SettingsField :label="t('settings2.recording.segment')">
                        <input
                            v-model.number="configStore.playout.recording.segment_duration"
                            type="number"
                            min="30"
                            max="3600"
                            step="1"
                            class="input input-sm w-full"
                        />
                    </SettingsField>
                    <SettingsField :label="t('settings2.recording.freeSpace')">
                        <input
                            v-model.number="configStore.playout.recording.minimum_free_space_gb"
                            type="number"
                            min="0"
                            step="1"
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
