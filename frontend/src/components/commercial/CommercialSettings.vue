<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'
import { useFolderUpload } from '@/composables/commercialUpload'
import { useCommercialApi } from '@/composables/commercialApi'

const BRAND_FOLDER = 'marca'
const AUDIO_MAPPINGS: AudioMapping[] = ['auto', 'first_stream', 'first_two_mono', 'mix_all']

const { t } = useI18n()
const api = useCommercialApi()
const configStore = useConfig()
const indexStore = useIndex()
const { uploadToFolder, uploading, reset } = useFolderUpload()

const conversion = ref<CommercialSettings | null>(null)
const report = ref<CommercialReportSettings | null>(null)
const auto = ref<AutoPlanSettings | null>(null)
const autoResult = ref<AutoPlanRun[]>([])

function fail(error: unknown) {
    indexStore.msgAlert('error', error instanceof Error ? error.message : String(error), 5)
}

async function load() {
    try {
        const [settings, reportSettings, autoStatus] = await Promise.all([
            api.settings(),
            api.reportSettings(),
            api.autoPlan(),
        ])

        conversion.value = settings
        report.value = reportSettings
        auto.value = autoStatus.settings
    } catch (error) {
        fail(error)
    }
}

async function saveConversion() {
    if (!conversion.value) {
        return
    }

    try {
        conversion.value = await api.saveSettings(conversion.value)
        indexStore.msgAlert('success', t('commercial.saved'), 3)
    } catch (error) {
        fail(error)
    }
}

async function saveAuto() {
    if (!auto.value) {
        return
    }

    try {
        auto.value = await api.saveAutoPlan(auto.value)
        indexStore.msgAlert('success', t('commercial.saved'), 3)
    } catch (error) {
        fail(error)
    }
}

async function runAuto() {
    try {
        autoResult.value = await api.runAutoPlan()
    } catch (error) {
        fail(error)
    }
}

async function saveReport() {
    if (!report.value) {
        return
    }

    try {
        report.value = await api.saveReportSettings(report.value)
        indexStore.msgAlert('success', t('commercial.saved'), 3)
    } catch (error) {
        fail(error)
    }
}

function removeLogo() {
    if (report.value) {
        report.value.logo_path = ''
        void saveReport()
    }
}

async function onLogo(event: Event) {
    const file = (event.target as HTMLInputElement).files?.[0]

    if (!file || !report.value) {
        return
    }

    try {
        report.value.logo_path = await uploadToFolder(file, BRAND_FOLDER)
        report.value = await api.saveReportSettings(report.value)
        indexStore.msgAlert('success', t('commercial.settings.logoSaved'), 3)
    } catch (error) {
        fail(error)
    } finally {
        reset()
    }
}

onMounted(load)
watch(() => configStore.i, load)
</script>
<template>
    <div class="space-y-5">
        <section v-if="auto" class="card bg-base-100 shadow p-4 space-y-3">
            <h2 class="font-bold text-lg">{{ t('commercial.auto.title') }}</h2>
            <p class="text-sm opacity-70">{{ t('commercial.auto.help') }}</p>
            <label class="flex items-center gap-2 text-sm">
                <input v-model="auto.enabled" type="checkbox" class="toggle toggle-sm toggle-accent" />
                {{ t('commercial.auto.enabled') }}
            </label>
            <label v-if="auto.enabled" class="form-control max-w-xs">
                <span class="label-text mb-1">{{ t('commercial.auto.horizon') }}</span>
                <input
                    v-model.number="auto.horizon_days"
                    type="number"
                    min="1"
                    max="14"
                    class="input input-sm w-full"
                />
            </label>
            <p class="text-xs opacity-60">{{ t('commercial.auto.rules') }}</p>
            <div class="flex flex-wrap gap-2">
                <button class="btn btn-sm btn-accent" @click="saveAuto">{{ t('commercial.save') }}</button>
                <button class="btn btn-sm" :disabled="!auto.enabled" @click="runAuto">
                    {{ t('commercial.auto.runNow') }}
                </button>
            </div>
            <ul v-if="autoResult.length" class="text-sm divide-y divide-base-300">
                <li v-for="day in autoResult" :key="day.date" class="py-1">
                    {{ day.date }} — {{ t(`commercial.auto.state.${day.state}`) }}
                    <span v-if="day.state === 'failed'" class="text-error"> ({{ day.message }})</span>
                </li>
            </ul>
        </section>

        <section v-if="report" class="card bg-base-100 shadow p-4 space-y-3">
            <h2 class="font-bold text-lg">{{ t('commercial.settings.reportTitle') }}</h2>
            <p class="text-sm opacity-70">{{ t('commercial.settings.reportHelp') }}</p>
            <label class="form-control">
                <span class="label-text mb-1">{{ t('commercial.settings.stationName') }}</span>
                <input v-model="report.station_name" class="input input-sm w-full" maxlength="100" />
            </label>
            <label class="form-control">
                <span class="label-text mb-1">{{ t('commercial.settings.stationDetails') }}</span>
                <textarea v-model="report.station_details" class="textarea w-full" rows="3" maxlength="500" />
                <span class="text-xs opacity-60 mt-1">{{ t('commercial.settings.stationDetailsHelp') }}</span>
            </label>
            <label class="form-control">
                <span class="label-text mb-1">{{ t('commercial.settings.footer') }}</span>
                <input v-model="report.footer_note" class="input input-sm w-full" maxlength="300" />
            </label>
            <div class="form-control">
                <span class="label-text mb-1">{{ t('commercial.settings.logo') }}</span>
                <div class="text-sm opacity-70 mb-1">{{ report.logo_path || t('commercial.settings.noLogo') }}</div>
                <input
                    type="file"
                    accept=".png,.jpg,.jpeg"
                    class="file-input file-input-sm w-full"
                    :disabled="uploading"
                    @change="onLogo"
                />
                <span class="text-xs opacity-60 mt-1">{{ t('commercial.settings.logoHelp') }}</span>
            </div>
            <div class="flex gap-2">
                <button class="btn btn-sm btn-accent" @click="saveReport">{{ t('commercial.save') }}</button>
                <button v-if="report.logo_path" class="btn btn-sm" @click="removeLogo">
                    {{ t('commercial.settings.removeLogo') }}
                </button>
            </div>
        </section>

        <section v-if="conversion" class="card bg-base-100 shadow p-4 space-y-3">
            <h2 class="font-bold text-lg">{{ t('commercial.settings.conversionTitle') }}</h2>
            <p class="text-sm opacity-70">{{ t('commercial.settings.conversionHelp') }}</p>
            <label class="form-control">
                <span class="label-text mb-1">{{ t('commercial.settings.audio') }}</span>
                <select v-model="conversion.audio_mapping" class="select select-sm w-full">
                    <option v-for="mapping in AUDIO_MAPPINGS" :key="mapping" :value="mapping">
                        {{ t(`commercial.settings.mapping.${mapping}`) }}
                    </option>
                </select>
            </label>
            <label class="flex items-center gap-2 text-sm">
                <input v-model="conversion.loudness_enabled" type="checkbox" class="toggle toggle-sm toggle-accent" />
                {{ t('commercial.settings.loudness') }}
            </label>
            <div v-if="conversion.loudness_enabled" class="grid gap-3 sm:grid-cols-2">
                <label class="form-control">
                    <span class="label-text mb-1">{{ t('commercial.settings.target') }}</span>
                    <input
                        v-model.number="conversion.loudness_target_lufs"
                        type="number"
                        step="0.5"
                        class="input input-sm w-full"
                    />
                </label>
                <label class="form-control">
                    <span class="label-text mb-1">{{ t('commercial.settings.peak') }}</span>
                    <input
                        v-model.number="conversion.loudness_true_peak_dbtp"
                        type="number"
                        step="0.5"
                        class="input input-sm w-full"
                    />
                </label>
            </div>
            <p class="text-xs opacity-60">{{ t('commercial.settings.loudnessHelp') }}</p>
            <div>
                <button class="btn btn-sm btn-accent" @click="saveConversion">{{ t('commercial.save') }}</button>
            </div>
        </section>
    </div>
</template>
