<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import GenericModal from '@/components/utils/GenericModal.vue'

import { friendlyError } from '@/composables/friendlyError'
import { authFetch } from '@/composables/authFetch'
import { formatDuration } from '@/composables/commercialApi'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'

export interface PickedFile {
    path: string
    name: string
    duration: number
}

const props = defineProps<{
    show: boolean
    title?: string
    confirmLabel?: string
    /** Choose one file instead of several. */
    single?: boolean
    /** Only show files with these extensions (lower case, no dot). */
    extensions?: string[]
}>()
const emit = defineEmits<{
    (event: 'close'): void
    (event: 'pick', files: PickedFile[]): void
}>()

const { t } = useI18n()
const configStore = useConfig()
const indexStore = useIndex()

const current = ref('')
const folders = ref<string[]>([])
const files = ref<{ name: string; duration: number }[]>([])
const selected = ref<string[]>([])
const loading = ref(false)

const crumbs = computed(() => current.value.split('/').filter(Boolean))
const joinPath = (name: string) => (current.value ? `${current.value}/${name}` : name)

async function browse(path: string) {
    loading.value = true

    try {
        const data = await authFetch<{
            folders?: (string | { name: string })[]
            files?: { name: string; duration: number }[]
        }>(`/api/file/${configStore.channels[configStore.i]?.id}/browse`, {
            method: 'POST',
            headers: { 'content-type': 'application/json;charset=UTF-8' },
            body: JSON.stringify({ source: path, folders_only: false }),
        })

        current.value = path
        folders.value = (data.folders ?? []).map((folder) => (typeof folder === 'string' ? folder : folder.name))
        files.value = (data.files ?? []).filter(
            (file) => !props.extensions || props.extensions.includes(file.name.split('.').pop()?.toLowerCase() ?? ''),
        )
    } catch (error) {
        indexStore.msgAlert('error', friendlyError(error), 6)
    } finally {
        loading.value = false
    }
}

function goUp(index: number) {
    void browse(crumbs.value.slice(0, index).join('/'))
}

function confirmPick() {
    const picked = files.value
        .filter((file) => selected.value.includes(joinPath(file.name)))
        .map((file) => ({ path: joinPath(file.name), name: file.name, duration: file.duration }))

    selected.value = []
    emit('pick', picked)
}

watch(
    () => props.show,
    (visible) => {
        if (visible) {
            selected.value = []
            void browse('')
        }
    },
)
</script>
<template>
    <GenericModal
        :show="show"
        :title="title || t('commercial.grid.pickTitle')"
        :hide-buttons="true"
        :modal-action="() => emit('close')"
    >
        <div class="w-150 max-w-full space-y-3">
            <div class="text-sm breadcrumbs py-0">
                <ul>
                    <li><a class="cursor-pointer" @click="goUp(0)">/</a></li>
                    <li v-for="(crumb, index) in crumbs" :key="index">
                        <a class="cursor-pointer" @click="goUp(index + 1)">{{ crumb }}</a>
                    </li>
                </ul>
            </div>
            <div class="max-h-80 overflow-y-auto border border-base-300 rounded-md">
                <div v-if="loading" class="p-3 text-sm opacity-70">{{ t('commercial.loading') }}</div>
                <ul v-else class="divide-y divide-base-300 text-sm">
                    <li v-for="folder in folders" :key="`d-${folder}`">
                        <button class="w-full text-left px-3 py-2 hover:bg-base-200" @click="browse(joinPath(folder))">
                            <i class="bi bi-folder me-2" />{{ folder }}
                        </button>
                    </li>
                    <li v-for="file in files" :key="`f-${file.name}`">
                        <label class="flex items-center gap-2 px-3 py-2 hover:bg-base-200 cursor-pointer">
                            <input
                                v-if="single"
                                type="radio"
                                class="radio radio-sm"
                                :checked="selected[0] === joinPath(file.name)"
                                @change="selected = [joinPath(file.name)]"
                            />
                            <input
                                v-else
                                v-model="selected"
                                type="checkbox"
                                class="checkbox checkbox-sm"
                                :value="joinPath(file.name)"
                            />
                            <span class="grow truncate">{{ file.name }}</span>
                            <span v-if="file.duration" class="opacity-60 tabular-nums">{{
                                formatDuration(file.duration)
                            }}</span>
                        </label>
                    </li>
                    <li v-if="!folders.length && !files.length" class="p-3 opacity-70">
                        {{ t('commercial.grid.emptyFolder') }}
                    </li>
                </ul>
            </div>
            <div class="flex justify-end gap-2">
                <button class="btn btn-sm" @click="emit('close')">{{ t('cancel') }}</button>
                <button class="btn btn-sm btn-accent" :disabled="!selected.length" @click="confirmPick">
                    {{ confirmLabel || t('commercial.grid.add', { count: selected.length }) }}
                </button>
            </div>
        </div>
    </GenericModal>
</template>
