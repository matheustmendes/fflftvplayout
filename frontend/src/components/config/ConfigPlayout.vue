<script setup lang="ts">
import { computed, ref, watch } from 'vue'
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
import { useIndex } from '@/stores/index'
import { useConfig } from '@/stores/config'

const { t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const dirty = usePlayoutDirty()

const saving = ref(false)
const showDiscardModal = ref(false)

const logLevels = ['INFO', 'WARNING', 'ERROR']
const notificationLevels = ['INFO', 'WARNING', 'ERROR', 'FATAL']
const processingMode = ['playlist', 'folder']

function outputMode(value: string | undefined): 'desktop' | 'hls' | 'stream' {
    if (value === 'desktop' || value === 'hls' || value === 'stream') {
        return value
    }

    return 'stream'
}

function deliveryLabel(name: string): string {
    if (name === 'hls' || name === 'stream' || name === 'desktop') {
        return t(`settings2.output.delivery.${name}`)
    }

    return name
}

const extensions = computed({
    get() {
        return configStore.playout.storage.extensions.join(',')
    },

    set(value: string) {
        configStore.playout.storage.extensions = value.replace(' ', '').split(/,|;/)
    },
})

const output = computed(() => configStore.playout.output.mode)

function addLiveListener() {
    const listeners = configStore.playout.ingest.listeners
    listeners.push({
        id: 0,
        priority: 0,
        enabled: false,
        name: '',
        backend: 'srt',
        identifier: 'srt://127.0.0.1:9000',
        options: {},
        demuxer_options: {},
    })
}

function removeLiveListener(index: number) {
    configStore.playout.ingest.listeners.splice(index, 1)
}

function changeLiveBackend(index: number) {
    const listener = configStore.playout.ingest.listeners[index]
    if (!listener) return

    listener.identifier = listener.backend === 'srt' ? 'srt://127.0.0.1:9000' : 'rtmp://127.0.0.1:1936/live/stream'
    listener.options = {}
    listener.demuxer_options = {}
}

const forceCustomResolution = ref(false)

const outputId = computed({
    get() {
        return configStore.playout.output.id
    },

    set(id: number) {
        const selected = configStore.outputs.find((output) => output.id === id)
        if (!selected) {
            return
        }

        forceCustomResolution.value = false

        configStore.playout.output.id = selected.id
        configStore.playout.output.mode = outputMode(selected.name)
        configStore.playout.output.stream_url = selected.stream_url
        configStore.playout.output.stream_type = selected.stream_type ?? 'rtmp'
        configStore.playout.output.stream_format = selected.stream_format ?? ''
        configStore.playout.output.hls_playlist_name = selected.hls_playlist_name ?? 'stream'
        configStore.playout.output.hls_segment_duration = selected.hls_segment_duration ?? 6
        configStore.playout.output.hls_list_size = selected.hls_list_size ?? 600
        configStore.playout.output.desktop_fullscreen = selected.desktop_fullscreen
        configStore.playout.output.width = selected.width
        configStore.playout.output.height = selected.height
        configStore.playout.output.fps = selected.fps
        configStore.playout.output.video_codec = selected.video_codec ?? 'libx264'
        try {
            configStore.playout.output.video_options = JSON.parse(selected.video_options || '{}')
        } catch {
            configStore.playout.output.video_options = {}
        }
        try {
            configStore.playout.output.protocol_options = JSON.parse(selected.protocol_options || '{}')
        } catch {
            configStore.playout.output.protocol_options = {}
        }
        try {
            configStore.playout.output.muxer_options = JSON.parse(selected.muxer_options || '{}')
        } catch {
            configStore.playout.output.muxer_options = {}
        }
        try {
            configStore.playout.output.metadata_options = JSON.parse(selected.metadata_options || '{}')
        } catch {
            configStore.playout.output.metadata_options = {}
        }
        configStore.playout.output.audio_codec = selected.audio_codec ?? 'aac'
        try {
            configStore.playout.output.audio_options = JSON.parse(selected.audio_options || '{}')
        } catch {
            configStore.playout.output.audio_options = {}
        }
        configStore.playout.output.audio_bitrate = selected.audio_bitrate ?? 128
        configStore.playout.output.hls_variants = (selected.hls_variants ?? '')
            .split(';')
            .map((v) => v.trim())
            .filter((v) => v.length > 0)
    },
})

const hasSelectedOutput = computed(() => configStore.outputs.some((output) => output.id === outputId.value))

const resolutionPresets = [
    { key: 'p1080', width: 1920, height: 1080 },
    { key: 'p720', width: 1280, height: 720 },
    { key: 'p480', width: 854, height: 480 },
    { key: 'p360', width: 640, height: 360 },
]

// A friendly picture size that falls back to "custom" (width and height boxes) for any other value.
const resolutionChoice = computed({
    get() {
        if (forceCustomResolution.value) {
            return 'custom'
        }

        const { width, height } = configStore.playout.output
        const preset = resolutionPresets.find((item) => item.width === width && item.height === height)

        return preset?.key ?? 'custom'
    },

    set(key: string) {
        const preset = resolutionPresets.find((item) => item.key === key)
        if (!preset) {
            forceCustomResolution.value = true
            return
        }

        forceCustomResolution.value = false
        configStore.playout.output.width = preset.width
        configStore.playout.output.height = preset.height
    },
})

interface HlsVariantRow {
    name: string
    width: string
    height: string
    videoBitrate: string
    audioBitrate: string
}

function parseHlsVariant(spec: string): HlsVariantRow {
    const [name = '', resolution = '', videoBitrate = '', audioBitrate = ''] = spec.split(':')
    const [width = '', height = ''] = resolution.split('x')

    return { name, width, height, videoBitrate, audioBitrate }
}

function serializeHlsVariant(row: HlsVariantRow): string {
    const base = `${row.name}:${row.width}x${row.height}:${row.videoBitrate}`
    return row.audioBitrate ? `${base}:${row.audioBitrate}` : base
}

const hlsVariants = computed<HlsVariantRow[]>({
    get() {
        return configStore.playout.output.hls_variants.map(parseHlsVariant)
    },

    set(rows: HlsVariantRow[]) {
        configStore.playout.output.hls_variants = rows.map(serializeHlsVariant)
    },
})

const codecOptions = computed<OutputCodecOptions>(() => {
    if (output.value === 'stream') {
        return configStore.outputCodecs[configStore.playout.output.stream_type ?? 'rtmp']
    }

    return configStore.outputCodecs.hls
})

function codecLabel(codec: CodecOption): string {
    const label = codec.display_name || codec.name
    const details = codec.hardware ? `${codec.codec_id}, hardware` : codec.codec_id

    return `${codec.name} - ${label} (${details})`
}

const videoSettings = computed(
    () =>
        codecOptions.value.video.find((codec) => codec.name === configStore.playout.output.video_codec)?.settings ?? [],
)
const audioUsesBitrate = computed(
    () =>
        codecOptions.value.audio.find((codec) => codec.name === configStore.playout.output.audio_codec)?.uses_bitrate ??
        true,
)

function settingIsVisible(setting: EncoderSetting): boolean {
    const condition = setting.visible_when
    return !condition || configStore.playout.output.video_options[condition.key] === condition.value
}

function normalizeVideoOptions() {
    const options: Record<string, string> = {}
    for (const setting of videoSettings.value) {
        options[setting.key] = configStore.playout.output.video_options?.[setting.key] ?? setting.default
    }
    configStore.playout.output.video_options = options
}

function setVideoOption(key: string, value: string | number) {
    configStore.playout.output.video_options = {
        ...configStore.playout.output.video_options,
        [key]: String(value),
    }
}

function setAudioCodec() {
    configStore.playout.output.audio_options = {}
}

function eventValue(event: Event): string {
    return (event.target as HTMLInputElement).value
}

watch(
    () => [
        output.value,
        configStore.playout.output.stream_type,
        configStore.playout.output.video_codec,
        codecOptions.value,
    ],
    () => {
        const video = codecOptions.value.video
        const audio = codecOptions.value.audio

        if (video.length > 0 && !video.some((codec) => codec.name === configStore.playout.output.video_codec)) {
            configStore.playout.output.video_codec = video[0].name
        }
        if (audio.length > 0 && !audio.some((codec) => codec.name === configStore.playout.output.audio_codec)) {
            configStore.playout.output.audio_codec = audio[0].name
            setAudioCodec()
        }
        normalizeVideoOptions()
    },
    { immediate: true },
)

function addHlsVariant() {
    hlsVariants.value = [
        ...hlsVariants.value,
        { name: '', width: '1280', height: '720', videoBitrate: '2500k', audioBitrate: '128k' },
    ]
}

function removeHlsVariant(index: number) {
    const rows = [...hlsVariants.value]
    rows.splice(index, 1)
    hlsVariants.value = rows
}

function updateHlsVariant(index: number, field: keyof HlsVariantRow, value: string) {
    const rows = [...hlsVariants.value]
    rows[index] = { ...rows[index], [field]: value }
    hlsVariants.value = rows
}

const formatIgnoreLines = computed({
    get() {
        return configStore.playout.logging.ignore_lines.join(';')
    },

    set(value) {
        configStore.playout.logging.ignore_lines = value.split(';')
    },
})

function discardChanges() {
    configStore.playout = cloneDeep(configStore.playoutSaved)
    forceCustomResolution.value = false
    showDiscardModal.value = false
}

async function onSubmitPlayout() {
    if (saving.value) {
        return
    }

    saving.value = true

    try {
        const update = await configStore.setPlayoutConfig(configStore.playout)
        configStore.onetimeInfo = true
        const { requires_restart: requiresRestart } = update

        indexStore.msgAlert('success', t('settings2.output.saved'), 2)

        const id = configStore.channels[configStore.i]?.id
        const response = await authFetch<string>(`/api/control/${id}/process`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({ command: 'status' }),
        })
        if (response === 'active' && requiresRestart) {
            configStore.showRestartModal = true
        }

        await configStore.getPlayoutConfig()
        await configStore.getPlayoutOutputs()
    } catch (error) {
        const detail = error instanceof Error ? error.message : String(error)
        indexStore.msgAlert('error', `${t('settings2.output.saveFailed')} ${detail}`, 4)
    } finally {
        saving.value = false
    }
}
</script>
<template>
    <form v-if="configStore.playout" class="space-y-5" @submit.prevent="onSubmitPlayout">
        <SettingsCard
            icon="bi-broadcast"
            :title="t('settings2.output.delivery.title')"
            :text="t('settings2.output.delivery.text')"
        >
            <SettingsField
                v-if="hasSelectedOutput"
                :label="t('settings2.output.delivery.label')"
                :help="t('settings2.output.delivery.help')"
            >
                <select v-model.number="outputId" class="select w-full">
                    <option v-for="item in configStore.outputs" :key="item.id" :value="item.id">
                        {{ deliveryLabel(item.name) }}
                    </option>
                </select>
            </SettingsField>

            <template v-if="output === 'stream'">
                <div class="grid gap-4 sm:grid-cols-2">
                    <SettingsField
                        :label="t('settings2.output.delivery.protocol')"
                        :help="t('settings2.output.delivery.protocolHelp')"
                    >
                        <select v-model="configStore.playout.output.stream_type" class="select w-full">
                            <option value="rtmp">RTMP</option>
                            <option value="srt">SRT</option>
                            <option value="udp">UDP</option>
                            <option value="custom">{{ t('settings2.output.delivery.custom') }}</option>
                        </select>
                    </SettingsField>
                    <SettingsField
                        v-if="configStore.playout.output.stream_type === 'custom'"
                        :label="t('settings2.output.delivery.customFormat')"
                    >
                        <input
                            v-model="configStore.playout.output.stream_format"
                            type="text"
                            class="input w-full"
                            placeholder="decklink"
                        />
                    </SettingsField>
                </div>
                <SettingsField
                    :label="
                        configStore.playout.output.stream_type === 'custom'
                            ? t('settings2.output.delivery.customTarget')
                            : t('settings2.output.delivery.destination')
                    "
                    :help="t('settings2.output.delivery.destinationHelp')"
                    :hint="t('settings2.output.delivery.destinationHint')"
                >
                    <input
                        v-model="configStore.playout.output.stream_url"
                        :type="configStore.playout.output.stream_type === 'custom' ? 'text' : 'url'"
                        class="input w-full"
                        placeholder="rtmp://servidor/canal/chave"
                    />
                </SettingsField>
            </template>

            <label v-if="output === 'desktop'" class="flex cursor-pointer items-center gap-3">
                <input
                    v-model="configStore.playout.output.desktop_fullscreen"
                    type="checkbox"
                    class="toggle toggle-primary"
                />
                <span class="font-semibold">{{ t('settings2.output.delivery.fullscreen') }}</span>
            </label>
        </SettingsCard>

        <SettingsCard
            icon="bi-aspect-ratio"
            :title="t('settings2.output.quality.title')"
            :text="t('settings2.output.quality.text')"
        >
            <SettingsField :label="t('settings2.output.quality.label')" :help="t('settings2.output.quality.help')">
                <select v-model="resolutionChoice" class="select w-full max-w-md">
                    <option v-for="preset in resolutionPresets" :key="preset.key" :value="preset.key">
                        {{ t(`settings2.output.quality.${preset.key}`) }}
                    </option>
                    <option value="custom">{{ t('settings2.output.quality.custom') }}</option>
                </select>
            </SettingsField>
            <div v-if="resolutionChoice === 'custom'" class="grid gap-4 sm:grid-cols-2 max-w-md">
                <SettingsField :label="t('settings2.output.quality.width')">
                    <input
                        v-model.number="configStore.playout.output.width"
                        type="number"
                        min="1"
                        step="1"
                        class="input w-full"
                    />
                </SettingsField>
                <SettingsField :label="t('settings2.output.quality.height')">
                    <input
                        v-model.number="configStore.playout.output.height"
                        type="number"
                        min="1"
                        step="1"
                        class="input w-full"
                    />
                </SettingsField>
            </div>
        </SettingsCard>

        <SettingsCard
            icon="bi-badge-tm"
            :title="t('settings2.output.logo.title')"
            :text="t('settings2.output.logo.text')"
        >
            <label class="flex cursor-pointer items-center gap-3">
                <input
                    v-model="configStore.playout.processing.add_logo"
                    type="checkbox"
                    class="toggle toggle-primary"
                />
                <span class="font-semibold">{{ t('settings2.output.logo.show') }}</span>
            </label>
            <SettingsField
                :label="t('settings2.output.logo.file')"
                :help="t('settings2.output.logo.help')"
                :hint="t('settings2.output.logo.hint')"
            >
                <input v-model="configStore.playout.processing.logo" type="text" name="logo" class="input w-full" />
            </SettingsField>
        </SettingsCard>

        <SettingsCard
            icon="bi-collection-play"
            :title="t('settings2.output.program.title')"
            :text="t('settings2.output.program.text')"
        >
            <SettingsField :label="t('settings2.output.program.mode')" :help="t('settings2.output.program.modeHelp')">
                <select v-model="configStore.playout.processing.mode" class="select w-full max-w-md">
                    <option v-for="mode in processingMode" :key="mode" :value="mode">
                        {{ t(`settings2.output.program.${mode}`) }}
                    </option>
                </select>
            </SettingsField>
            <SettingsField
                :label="t('settings2.output.program.filler')"
                :help="t('settings2.output.program.fillerHelp')"
            >
                <input v-model="configStore.playout.storage.filler" type="text" name="filler" class="input w-full" />
            </SettingsField>
        </SettingsCard>

        <AdvancedBlock>
            <SettingsGroup :title="t('settings2.output.adv.logo')">
                <p class="text-sm opacity-70">{{ t('config.processingLogoPath') }}</p>
                <div class="grid gap-3 sm:grid-cols-3">
                    <SettingsField
                        :label="t('settings2.output.adv.logoOpacity')"
                        :hint="t('settings2.output.adv.logoOpacityHint')"
                    >
                        <input
                            v-model="configStore.playout.processing.logo_opacity"
                            type="number"
                            min="0"
                            max="1"
                            step="0.01"
                            class="input input-sm w-full"
                        />
                    </SettingsField>
                    <SettingsField :label="t('settings2.output.adv.logoScale')" :hint="t('config.processingLogoScale')">
                        <input
                            v-model="configStore.playout.processing.logo_scale"
                            type="text"
                            name="logo_scale"
                            class="input input-sm w-full"
                        />
                    </SettingsField>
                    <SettingsField
                        :label="t('settings2.output.adv.logoPosition')"
                        :hint="t('config.processingLogoPosition')"
                    >
                        <input
                            v-model="configStore.playout.processing.logo_position"
                            type="text"
                            name="logo_position"
                            class="input input-sm w-full"
                        />
                    </SettingsField>
                </div>
            </SettingsGroup>

            <SettingsGroup :title="t('settings2.output.adv.video')" :text="t('settings2.output.adv.videoText')">
                <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
                    <SettingsField :label="t('settings2.output.adv.fps')">
                        <input
                            v-model.number="configStore.playout.output.fps"
                            type="number"
                            min="1"
                            step="0.01"
                            class="input input-sm w-full"
                        />
                    </SettingsField>
                    <template v-if="output === 'hls' || output === 'stream'">
                        <SettingsField :label="t('config.videoCodec')">
                            <select v-model="configStore.playout.output.video_codec" class="select select-sm w-full">
                                <option v-for="codec in codecOptions.video" :key="codec.name" :value="codec.name">
                                    {{ codecLabel(codec) }}
                                </option>
                            </select>
                        </SettingsField>
                        <SettingsField :label="t('config.audioCodec')">
                            <select
                                v-model="configStore.playout.output.audio_codec"
                                class="select select-sm w-full"
                                @change="setAudioCodec"
                            >
                                <option v-for="codec in codecOptions.audio" :key="codec.name" :value="codec.name">
                                    {{ codecLabel(codec) }}
                                </option>
                            </select>
                        </SettingsField>
                        <template v-for="setting in videoSettings" :key="setting.key">
                            <SettingsField v-if="settingIsVisible(setting)" :label="setting.label">
                                <select
                                    v-if="setting.kind === 'select'"
                                    :value="configStore.playout.output.video_options[setting.key]"
                                    class="select select-sm w-full"
                                    @change="setVideoOption(setting.key, eventValue($event))"
                                >
                                    <option v-for="choice in setting.choices" :key="choice.value" :value="choice.value">
                                        {{ choice.label }}
                                    </option>
                                </select>
                                <input
                                    v-else
                                    :value="configStore.playout.output.video_options[setting.key]"
                                    type="number"
                                    :min="setting.minimum ?? undefined"
                                    :max="setting.maximum ?? undefined"
                                    step="1"
                                    class="input input-sm w-full"
                                    @input="setVideoOption(setting.key, eventValue($event))"
                                />
                            </SettingsField>
                        </template>
                        <SettingsField v-if="audioUsesBitrate" :label="t('config.audioBitrate')">
                            <input
                                v-model.number="configStore.playout.output.audio_bitrate"
                                type="number"
                                min="1"
                                class="input input-sm w-full"
                            />
                        </SettingsField>
                    </template>
                </div>
            </SettingsGroup>

            <SettingsGroup
                v-if="output === 'hls' || output === 'stream'"
                :title="t('settings2.output.adv.outputExtra')"
            >
                <SettingsField :label="t('config.audioEncoderOptions')" :hint="t('config.audioEncoderOptionsHelp')">
                    <KeyValueEditor
                        :key="`audio-${outputId}`"
                        v-model="configStore.playout.output.audio_options"
                        :name-placeholder="t('config.audioOptionName')"
                        :value-placeholder="t('config.audioOptionValue')"
                        :add-label="t('config.addAudioOption')"
                    />
                </SettingsField>
                <SettingsField :label="t('config.muxerOptions')" :hint="t('config.muxerOptionsHelp')">
                    <KeyValueEditor
                        :key="`muxer-${outputId}`"
                        v-model="configStore.playout.output.muxer_options"
                        :name-placeholder="t('config.muxerOptionName')"
                        :value-placeholder="t('config.muxerOptionValue')"
                        :add-label="t('config.addMuxerOption')"
                    />
                </SettingsField>
                <SettingsField :label="t('config.outputMetadata')" :hint="t('config.outputMetadataHelp')">
                    <KeyValueEditor
                        :key="`metadata-${outputId}`"
                        v-model="configStore.playout.output.metadata_options"
                        :name-placeholder="t('config.metadataKey')"
                        :value-placeholder="t('config.metadataValue')"
                        :add-label="t('config.addMetadata')"
                    />
                </SettingsField>
                <SettingsField
                    v-if="output === 'stream'"
                    :label="t('config.protocolOptions')"
                    :hint="t('config.protocolOptionsHelp')"
                >
                    <KeyValueEditor
                        :key="`protocol-${outputId}`"
                        v-model="configStore.playout.output.protocol_options"
                        :trim-values="false"
                        :name-placeholder="t('config.protocolOptionName')"
                        :value-placeholder="t('config.protocolOptionValue')"
                        :add-label="t('config.addProtocolOption')"
                    />
                </SettingsField>
            </SettingsGroup>

            <SettingsGroup v-if="output === 'hls'" :title="t('settings2.output.adv.hls')">
                <div class="grid gap-3 sm:grid-cols-3">
                    <SettingsField :label="t('config.hlsPlaylistName')">
                        <input
                            v-model.trim="configStore.playout.output.hls_playlist_name"
                            type="text"
                            class="input input-sm w-full"
                        />
                    </SettingsField>
                    <SettingsField :label="t('config.hlsSegmentDuration')">
                        <input
                            v-model.number="configStore.playout.output.hls_segment_duration"
                            type="number"
                            min="1"
                            class="input input-sm w-full"
                        />
                    </SettingsField>
                    <SettingsField :label="t('config.hlsListSize')">
                        <input
                            v-model.number="configStore.playout.output.hls_list_size"
                            type="number"
                            min="0"
                            class="input input-sm w-full"
                        />
                    </SettingsField>
                </div>
                <p class="text-sm opacity-70">{{ t('config.outputParam') }}</p>

                <SettingsField :label="t('config.hlsVariants')" :hint="t('config.hlsVariantsHelp')">
                    <div v-for="(variant, index) in hlsVariants" :key="index" class="flex flex-wrap items-center gap-2">
                        <input
                            :value="variant.name"
                            type="text"
                            :placeholder="t('settings2.output.adv.variantName')"
                            class="input input-sm w-24"
                            @input="updateHlsVariant(index, 'name', eventValue($event))"
                        />
                        <input
                            :value="variant.width"
                            type="number"
                            :placeholder="t('settings2.output.quality.width')"
                            class="input input-sm w-24"
                            @input="updateHlsVariant(index, 'width', eventValue($event))"
                        />
                        <span>x</span>
                        <input
                            :value="variant.height"
                            type="number"
                            :placeholder="t('settings2.output.quality.height')"
                            class="input input-sm w-24"
                            @input="updateHlsVariant(index, 'height', eventValue($event))"
                        />
                        <input
                            :value="variant.videoBitrate"
                            type="text"
                            :placeholder="t('settings2.output.adv.variantVideoBitrate')"
                            class="input input-sm w-36"
                            @input="updateHlsVariant(index, 'videoBitrate', eventValue($event))"
                        />
                        <input
                            :value="variant.audioBitrate"
                            type="text"
                            :placeholder="t('settings2.output.adv.variantAudioBitrate')"
                            class="input input-sm w-36"
                            @input="updateHlsVariant(index, 'audioBitrate', eventValue($event))"
                        />
                        <button type="button" class="btn btn-sm btn-error btn-outline" @click="removeHlsVariant(index)">
                            {{ t('config.remove') }}
                        </button>
                    </div>
                    <div>
                        <button type="button" class="btn btn-sm btn-outline" @click="addHlsVariant">
                            {{ t('config.addHlsVariant') }}
                        </button>
                    </div>
                </SettingsField>
            </SettingsGroup>

            <SettingsGroup :title="t('settings2.output.adv.ingest')" :text="t('config.ingestHelp')">
                <div
                    v-for="(listener, index) in configStore.playout.ingest.listeners"
                    :key="listener.id || `new-${index}`"
                    class="rounded-box border border-base-300 bg-base-100 p-3 space-y-3"
                >
                    <div class="flex items-center justify-between gap-3">
                        <label class="flex cursor-pointer items-center gap-2">
                            <input v-model="listener.enabled" type="checkbox" class="checkbox" />
                            {{ t('settings2.output.adv.listenerEnabled') }}
                        </label>
                        <button type="button" class="btn btn-ghost btn-sm" @click="removeLiveListener(index)">
                            {{ t('config.liveListenerRemove') }}
                        </button>
                    </div>
                    <div class="grid gap-3 md:grid-cols-2">
                        <SettingsField :label="t('config.liveListenerName')">
                            <input v-model="listener.name" type="text" class="input input-sm w-full" />
                        </SettingsField>
                        <SettingsField :label="t('config.liveListenerBackend')">
                            <select
                                v-model="listener.backend"
                                class="select select-sm w-full"
                                @change="changeLiveBackend(index)"
                            >
                                <option value="rtmp">RTMP</option>
                                <option value="srt">SRT</option>
                            </select>
                        </SettingsField>
                        <SettingsField :label="t('config.liveListenerUrl')">
                            <input v-model="listener.identifier" type="text" class="input input-sm w-full" />
                        </SettingsField>
                        <SettingsField :label="t('config.liveListenerPriority')">
                            <input
                                v-model.number="listener.priority"
                                type="number"
                                min="0"
                                max="100"
                                class="input input-sm w-full"
                            />
                        </SettingsField>
                    </div>
                    <details
                        class="rounded-box border border-base-300 p-3"
                        :open="
                            Object.keys(listener.options).length !== 0 ||
                            Object.keys(listener.demuxer_options).length !== 0
                        "
                    >
                        <summary class="cursor-pointer font-semibold">{{ t('player.advanced') }}</summary>
                        <div class="mt-3 space-y-4">
                            <SettingsField
                                :label="t('config.liveListenerOptions')"
                                :hint="t('config.liveListenerOptionsHelp')"
                            >
                                <KeyValueEditor
                                    v-model="listener.options"
                                    :trim-values="false"
                                    :name-placeholder="t('config.protocolOptionName')"
                                    :value-placeholder="t('config.protocolOptionValue')"
                                    :add-label="t('config.addProtocolOption')"
                                />
                            </SettingsField>
                            <SettingsField
                                :label="t('config.liveDemuxerOptions')"
                                :hint="t('config.liveDemuxerOptionsHelp')"
                            >
                                <KeyValueEditor
                                    v-model="listener.demuxer_options"
                                    :trim-values="false"
                                    :name-placeholder="t('config.protocolOptionName')"
                                    :value-placeholder="t('config.protocolOptionValue')"
                                    :add-label="t('config.addDemuxerOption')"
                                />
                            </SettingsField>
                        </div>
                    </details>
                </div>
                <div>
                    <button type="button" class="btn btn-sm btn-outline" @click="addLiveListener">
                        {{ t('config.liveListenerAdd') }}
                    </button>
                </div>
            </SettingsGroup>

            <SettingsGroup :title="t('settings2.output.adv.playlist')" :text="t('config.playlistHelp')">
                <div class="grid gap-3 sm:grid-cols-2">
                    <SettingsField :label="t('settings2.output.adv.dayStart')" :hint="t('config.playlistDayStart')">
                        <input
                            v-model="configStore.playout.playlist.day_start"
                            type="text"
                            name="day_start"
                            class="input input-sm w-full"
                            pattern="(([01]?[0-9]|2[0-4]):[0-5][0-9]:[0-5][0-9]|now|none)"
                        />
                    </SettingsField>
                    <SettingsField :label="t('settings2.output.adv.length')" :hint="t('config.playlistLength')">
                        <input
                            v-model="configStore.playout.playlist.length"
                            type="text"
                            name="length"
                            class="input input-sm w-full"
                            pattern="([01]?[0-9]|2[0-4]):[0-5][0-9]:[0-5][0-9]"
                        />
                    </SettingsField>
                </div>
                <label class="flex cursor-pointer items-center gap-2">
                    <input v-model="configStore.playout.playlist.infinit" type="checkbox" class="checkbox" />
                    {{ t('settings2.output.adv.infinit') }}
                </label>
                <p class="text-sm opacity-70">{{ t('config.playlistInfinit') }}</p>
            </SettingsGroup>

            <SettingsGroup :title="t('settings2.output.adv.storage')" :text="t('config.storageHelp')">
                <SettingsField :label="t('settings2.output.adv.extensions')" :hint="t('config.storageExtension')">
                    <input v-model="extensions" type="text" name="extensions" class="input input-sm w-full" />
                </SettingsField>
                <label class="flex cursor-pointer items-center gap-2">
                    <input v-model="configStore.playout.storage.shuffle" type="checkbox" class="checkbox" />
                    {{ t('settings2.output.adv.shuffle') }}
                </label>
                <p class="text-sm opacity-70">{{ t('config.storageShuffle') }}</p>
                <p class="text-sm opacity-70">{{ t('config.storageFiller') }}</p>
            </SettingsGroup>

            <SettingsGroup :title="t('settings2.output.adv.subtitles')">
                <label class="flex cursor-pointer items-center gap-2">
                    <input v-model="configStore.playout.processing.vtt_enable" type="checkbox" class="checkbox" />
                    {{ t('settings2.output.adv.vttEnable') }}
                </label>
                <p class="text-sm opacity-70">{{ t('config.processingVTTEnable') }}</p>
                <SettingsField :label="t('settings2.output.adv.vttDummy')" :hint="t('config.processingVTTDummy')">
                    <input
                        v-model="configStore.playout.processing.vtt_dummy"
                        type="text"
                        name="vtt_dummy"
                        class="input input-sm w-full"
                    />
                </SettingsField>
                <div v-if="configStore.playout.processing.vtt_enable" class="grid gap-3 sm:grid-cols-3">
                    <SettingsField :label="t('config.hlsSubtitleName')">
                        <input
                            v-model.trim="configStore.playout.processing.vtt_name"
                            type="text"
                            class="input input-sm w-full"
                        />
                    </SettingsField>
                    <SettingsField :label="t('config.hlsSubtitleLanguage')">
                        <input
                            v-model.trim="configStore.playout.processing.vtt_language"
                            type="text"
                            class="input input-sm w-full"
                        />
                    </SettingsField>
                    <SettingsField :label="t('config.hlsSubtitleDefault')">
                        <input v-model="configStore.playout.processing.vtt_default" type="checkbox" class="toggle" />
                    </SettingsField>
                </div>
            </SettingsGroup>

            <SettingsGroup :title="t('settings2.output.adv.task')" :text="t('config.taskHelp')">
                <label class="flex cursor-pointer items-center gap-2">
                    <input v-model="configStore.playout.task.enable" type="checkbox" class="checkbox" />
                    {{ t('settings2.output.adv.taskEnable') }}
                </label>
                <SettingsField :label="t('settings2.output.adv.taskPath')" :hint="t('config.taskPath')">
                    <input
                        v-model="configStore.playout.task.path"
                        type="text"
                        name="task_path"
                        class="input input-sm w-full"
                    />
                </SettingsField>
            </SettingsGroup>

            <SettingsGroup :title="t('settings2.output.adv.log')" :text="t('config.logHelp')">
                <div class="grid gap-3 sm:grid-cols-2">
                    <SettingsField :label="t('settings2.output.adv.ffmpegLevel')">
                        <select v-model="configStore.playout.logging.ffmpeg_level" class="select select-sm w-full">
                            <option v-for="level in logLevels" :key="level" :value="level">{{ level }}</option>
                        </select>
                    </SettingsField>
                    <SettingsField :label="t('settings2.output.adv.ingestLevel')">
                        <select v-model="configStore.playout.logging.ingest_level" class="select select-sm w-full">
                            <option v-for="level in logLevels" :key="level" :value="level">{{ level }}</option>
                        </select>
                    </SettingsField>
                </div>
                <label class="flex cursor-pointer items-center gap-2">
                    <input v-model="configStore.playout.logging.detect_silence" type="checkbox" class="checkbox" />
                    {{ t('settings2.output.adv.detectSilence') }}
                </label>
                <p class="text-sm opacity-70">{{ t('config.logDetect') }}</p>
                <SettingsField :label="t('settings2.output.adv.ignoreLines')" :hint="t('config.logIgnore')">
                    <input v-model="formatIgnoreLines" type="text" class="input input-sm w-full truncate" />
                </SettingsField>
            </SettingsGroup>

            <SettingsGroup :title="t('settings2.output.adv.general')" :text="t('config.generalHelp')">
                <SettingsField :label="t('settings2.output.adv.stopThreshold')" :hint="t('config.stopThreshold')">
                    <input
                        v-model="configStore.playout.general.stop_threshold"
                        type="number"
                        min="3"
                        class="input input-sm w-full max-w-36"
                    />
                </SettingsField>
            </SettingsGroup>

            <SettingsGroup
                v-if="configStore.playout.mail.show"
                :title="t('settings2.output.adv.mail')"
                :text="t('config.mailHelp')"
            >
                <div class="grid gap-3 sm:grid-cols-2">
                    <SettingsField :label="t('settings2.output.adv.mailSubject')">
                        <input
                            v-model="configStore.playout.mail.subject"
                            type="text"
                            name="subject"
                            class="input input-sm w-full"
                        />
                    </SettingsField>
                    <SettingsField :label="t('settings2.output.adv.mailRecipient')">
                        <input
                            v-model="configStore.playout.mail.recipient"
                            type="text"
                            name="recipient"
                            class="input input-sm w-full"
                        />
                    </SettingsField>
                    <SettingsField :label="t('settings2.output.adv.mailLevel')">
                        <select v-model="configStore.playout.mail.mail_level" class="select select-sm w-full">
                            <option v-for="level in logLevels" :key="level" :value="level">{{ level }}</option>
                        </select>
                    </SettingsField>
                    <SettingsField :label="t('settings2.output.adv.mailInterval')" :hint="t('config.mailInterval')">
                        <input
                            v-model="configStore.playout.mail.interval"
                            type="number"
                            min="30"
                            step="10"
                            class="input input-sm w-full"
                        />
                    </SettingsField>
                </div>
            </SettingsGroup>

            <SettingsGroup
                v-if="configStore.playout.notification.show"
                :title="t('settings2.output.adv.notification')"
                :text="t('config.notificationHelp')"
            >
                <div class="grid gap-3 sm:grid-cols-2">
                    <SettingsField :label="t('config.notificationTopic')">
                        <input
                            v-model="configStore.playout.notification.topic"
                            type="text"
                            class="input input-sm w-full"
                        />
                    </SettingsField>
                    <SettingsField :label="t('config.notificationLevel')">
                        <select v-model="configStore.playout.notification.level" class="select select-sm w-full">
                            <option v-for="level in notificationLevels" :key="level" :value="level">{{ level }}</option>
                        </select>
                    </SettingsField>
                    <SettingsField :label="t('config.notificationTags')">
                        <input
                            v-model="configStore.playout.notification.tags"
                            type="text"
                            class="input input-sm w-full"
                        />
                    </SettingsField>
                </div>
            </SettingsGroup>
        </AdvancedBlock>

        <p class="flex items-start gap-2 text-sm opacity-70">
            <i class="bi bi-info-circle mt-0.5" />
            {{ t('settings2.output.restartNote') }}
        </p>
        <SaveBar :dirty="dirty" :saving="saving" @discard="showDiscardModal = true" />
    </form>

    <DiscardModal :show="showDiscardModal" @confirm="discardChanges" @cancel="showDiscardModal = false" />
    <RestartModal />
</template>
