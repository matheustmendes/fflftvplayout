<script setup lang="ts">
import { computed, nextTick, ref, onMounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { storeToRefs } from 'pinia'
import { useHead } from '@unhead/vue'

import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'
import { useConfig } from '@/stores/config'
import { useMedia } from '@/stores/media'

import GenericModal from '@/components/utils/GenericModal.vue'
import VideoPlayer from '@/components/utils/VideoPlayer.vue'
import ConfirmModal from '@/components/ui/ConfirmModal.vue'
import EmptyState from '@/components/ui/EmptyState.vue'
import HelpTip from '@/components/ui/HelpTip.vue'
import PageHeader from '@/components/ui/PageHeader.vue'

import { authFetch } from '@/composables/authFetch'
import { stringFormatter } from '@/composables/helper'
import { humanDuration, humanSize } from '@/composables/humanFormat'
import { createFilePreviewUrl } from '@/composables/fileAccess'
import { useFileUpload } from '@/composables/useFileUpload'

type UploadStage = 'choose' | 'sending' | 'done' | 'error'

const { t, locale } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const mediaStore = useMedia()
const { mediaType, filename, parent, dir_file } = stringFormatter()
const { i } = storeToRefs(useConfig())
const {
    uploadTask,
    currentNumber,
    currentProgress,
    resetUploadState,
    uploadFiles: uploadFilesComposable,
} = useFileUpload()

const deleteName = ref('')
const recursive = ref(false)
const renameOldName = ref('')
const renameOldPath = ref('')
const renameNewName = ref('')
const renameIsFolder = ref(false)
const renameError = ref('')
const previewName = ref('')
const previewUrl = ref('')
const previewOpt = ref()
const isVideo = ref(false)
const showDeleteModal = ref(false)
const showPreviewModal = ref(false)
const showRenameModal = ref(false)
const showCreateModal = ref(false)
const showUploadModal = ref(false)
const showOverwriteModal = ref(false)
const extensionsArr = ref([] as string[])
const newFolderName = ref('')
const createError = ref('')
const inputFiles = ref([] as File[])
const fileInput = ref<HTMLInputElement | null>(null)
const renameInput = ref<HTMLInputElement | null>(null)
const createInput = ref<HTMLInputElement | null>(null)
const lastPath = ref('')
const uploadStage = ref<UploadStage>('choose')
const uploadError = ref('')
const uploadedCount = ref(0)
const skippedNames = ref([] as string[])
const conflictNames = ref([] as string[])
const filterText = ref('')
const dropTarget = ref('')
const fileDragDepth = ref(0)
const uploadDropActive = ref(false)

const extensions = computed(() => extensionsArr.value.join(', '))
const isFileDragging = computed(() => fileDragDepth.value > 0)
const currentSource = computed(() => mediaStore.folderTree.source)
const canGoUp = computed(() => mediaStore.crumbs.length > 1)
const deleteIsFile = computed(() =>
    extensionsArr.value.some((suffix) => deleteName.value.toLowerCase().endsWith(suffix)),
)
const sending = computed(() => uploadStage.value === 'sending')
const percent = computed(() => Math.min(100, Math.round(currentProgress.value)))
const totalUploadSize = computed(() => inputFiles.value.reduce((sum, file) => sum + file.size, 0))

const query = computed(() => filterText.value.trim().toLowerCase())
const shownFolders = computed(() =>
    mediaStore.folderTree.folders.filter((folder) => !query.value || folder.name.toLowerCase().includes(query.value)),
)
const shownFiles = computed(() =>
    mediaStore.folderTree.files.filter((file) => !query.value || file.name.toLowerCase().includes(query.value)),
)
const isEmptyFolder = computed(
    () => mediaStore.folderTree.folders.length === 0 && mediaStore.folderTree.files.length === 0,
)
const countLabel = computed(() => {
    const folders = mediaStore.folderTree.folders.length
    const files = mediaStore.folderTree.files.length
    const parts = []

    if (folders > 0) {
        parts.push(t('files2.count', folders))
    }

    if (files > 0) {
        parts.push(t('files2.countFiles', files))
    }

    return parts.join(' · ')
})
const entryCount = computed(() => mediaStore.folderTree.folders.length + mediaStore.folderTree.files.length)

useHead({
    title: computed(() => t('nav.files')),
})

onMounted(async () => {
    const config_extensions = configStore.playout.storage.extensions
    let extra_extensions = configStore.channels[configStore.i]?.extra_extensions || []

    if (typeof extra_extensions === 'string') {
        extra_extensions = extra_extensions.split(',')
    }

    extensionsArr.value = [...config_extensions, ...extra_extensions].map((ext) => {
        return `.${ext}`.toLowerCase()
    })

    if (!mediaStore.folderTree.parent || !mediaStore.currentPath) {
        await mediaStore.getTree('')
    }
})

watch([i], () => {
    mediaStore.getTree('')
})

watch(currentSource, () => {
    filterText.value = ''
})

function pathOf(name: string): string {
    return `/${mediaStore.folderTree.source}/${name}`.replace(/\/[/]+/g, '/')
}

function openFolder(name: string) {
    mediaStore.getTree(pathOf(name))
}

function openParentFolder(name: string) {
    mediaStore.getTree(`/${parent(mediaStore.folderTree.source)}/${name}`)
}

function goUp() {
    const target = mediaStore.crumbs[mediaStore.crumbs.length - 2]

    if (target) {
        mediaStore.getTree(target.path)
    }
}

function typeLabel(name: string): string {
    switch (mediaType(name)) {
        case 'video':
            return t('files2.typeVideo')
        case 'audio':
            return t('files2.typeAudio')
        case 'image':
            return t('files2.typeImage')
        case 'live':
            return t('files2.typeLive')
        default:
            return t('files2.typeOther')
    }
}

function typeIcon(name: string): string {
    switch (mediaType(name)) {
        case 'video':
            return 'bi-film text-primary bg-primary/10'
        case 'audio':
            return 'bi-music-note-beamed text-warning bg-warning/15'
        case 'image':
            return 'bi-file-earmark-image text-success bg-success/10'
        default:
            return 'bi-file-earmark text-base-content/60 bg-base-200'
    }
}

/*
    Drag and drop: moving a file onto a folder (internal drag, JSON payload)
    and dropping files from the computer to upload them (external drag, "Files").
*/
function isExternalFileDrag(event: DragEvent): boolean {
    return Boolean(event.dataTransfer?.types.includes('Files'))
}

function handleDragStart(event: DragEvent, itemData: { name: string }) {
    event.dataTransfer?.setData('application/json', JSON.stringify(itemData))
}

function handleDragOver(event: DragEvent, key: string) {
    if (!isExternalFileDrag(event)) {
        dropTarget.value = key
    }
}

function handleDragLeave(key: string) {
    if (dropTarget.value === key) {
        dropTarget.value = ''
    }
}

async function handleDrop(event: DragEvent, targetFolder: any, isParent: boolean | null) {
    dropTarget.value = ''

    const raw = event.dataTransfer?.getData('application/json')

    if (!raw) {
        return
    }

    const itemData = JSON.parse(raw)
    const source = `/${mediaStore.folderTree.source}/${itemData.name}`.replace(/\/[/]+/g, '/')
    let target

    if (isParent === null) {
        target = `${targetFolder}/${itemData.name}`.replace(/\/[/]+/g, '/')
    } else if (isParent) {
        target = `/${parent(mediaStore.folderTree.source)}/${targetFolder.name}/${itemData.name}`.replace(
            /\/[/]+/g,
            '/',
        )
    } else {
        target = `/${mediaStore.folderTree.source}/${targetFolder.name}/${itemData.name}`.replace(/\/[/]+/g, '/')
    }

    if (source !== target) {
        await authFetch<void>(`/api/file/${configStore.channels[configStore.i]?.id}/rename`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({ source, target }),
        })
            .then(() => {
                indexStore.msgAlert('success', t('files2.moved', { name: itemData.name }), 3)
                mediaStore.getTree(mediaStore.folderTree.source)
            })
            .catch((e) => {
                indexStore.msgAlert('error', `${t('media.moveError')}: ${e}`, 3)
            })
    }
}

function onPageDragEnter(event: DragEvent) {
    if (isExternalFileDrag(event)) {
        fileDragDepth.value++
    }
}

function onPageDragLeave(event: DragEvent) {
    if (isExternalFileDrag(event)) {
        fileDragDepth.value = Math.max(0, fileDragDepth.value - 1)
    }
}

function onPageDrop(event: DragEvent) {
    fileDragDepth.value = 0

    if (isExternalFileDrag(event) && event.dataTransfer) {
        openUploadWith(Array.from(event.dataTransfer.files))
    }
}

async function setPreviewData(path: string) {
    /*
        Set path and player options for video preview.
    */
    let fullPath = path
    if (!path.includes('/')) {
        fullPath = `/${mediaStore.folderTree.parent}/${mediaStore.folderTree.source}/${path}`.replace(/\/[/]+/g, '/')
    }

    previewName.value = fullPath.split('/').slice(-1)[0] || ''

    try {
        previewUrl.value = await createFilePreviewUrl(configStore.channels[configStore.i]?.id, fullPath)
    } catch (error) {
        indexStore.msgAlert('error', error instanceof Error ? error.message : String(error), 5)
        return
    }

    const ext = previewName.value.split('.').slice(-1)[0]?.toLowerCase()
    const fileType =
        mediaType(previewName.value) === 'audio'
            ? `audio/${ext}`
            : mediaType(previewName.value) === 'live'
              ? 'application/x-mpegURL'
              : `video/${ext}`

    if (configStore.playout.storage.extensions.includes(`${ext}`)) {
        isVideo.value = true
        previewOpt.value = {
            liveui: false,
            controls: true,
            suppressNotSupportedError: true,
            autoplay: false,
            preload: 'auto',
            sources: [
                {
                    type: fileType,
                    src: previewUrl.value,
                },
            ],
        }
    } else {
        isVideo.value = false
    }
}

function openPreview(name: string) {
    showPreviewModal.value = true
    setPreviewData(name)
}

function askDelete(name: string) {
    deleteName.value = pathOf(name)
    recursive.value = false
    showDeleteModal.value = true
}

async function deleteFileOrFolder(del: boolean) {
    /*
        Delete function, works for files and folders.
    */
    showDeleteModal.value = false

    if (del) {
        await authFetch<void>(`/api/file/${configStore.channels[configStore.i]?.id}/remove`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({ source: deleteName.value, recursive: recursive.value }),
        })
            .then(() => {
                indexStore.msgAlert('success', t('files2.deleted', { name: dir_file(deleteName.value).file }), 3)
                mediaStore.getTree(mediaStore.folderTree.source)
            })
            .catch((e) => {
                indexStore.msgAlert('error', `${t('media.deleteError')}: ${e}`, 5)
            })

        recursive.value = false
    }

    deleteName.value = ''
}

function setRenameValues(name: string, isFolder: boolean) {
    const filepath = dir_file(pathOf(name))

    renameOldName.value = filepath.file
    renameOldPath.value = filepath.dir
    renameNewName.value = filepath.file
    renameIsFolder.value = isFolder
    renameError.value = ''
    showRenameModal.value = true

    nextTick(() => {
        renameInput.value?.focus()
        renameInput.value?.select()
    })
}

function closeRename() {
    showRenameModal.value = false
    renameOldName.value = ''
    renameOldPath.value = ''
    renameNewName.value = ''
    renameIsFolder.value = false
    renameError.value = ''
}

async function renameFile() {
    /*
        Form submit for file rename request.
    */
    const newName = renameNewName.value.trim()

    if (!newName || /[/\\]/.test(newName)) {
        renameError.value = t('files2.nameInvalid')
        return
    }

    if (newName !== renameOldName.value) {
        const nameTaken =
            mediaStore.folderTree.files.some((file) => file.name === newName) ||
            mediaStore.folderTree.folders.some((folder) => folder.name === newName)

        if (nameTaken) {
            renameError.value = t('files2.nameTaken')
            return
        }

        await authFetch<void>(`/api/file/${configStore.channels[configStore.i]?.id}/rename`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({
                source: `${renameOldPath.value}${renameOldName.value}`,
                target: `${renameOldPath.value}${newName}`,
            }),
        })
            .then(() => {
                indexStore.msgAlert('success', t('files2.renamed', { name: newName }), 3)
                mediaStore.getTree(mediaStore.folderTree.source)
            })
            .catch((e) => {
                indexStore.msgAlert('error', `${t('media.moveError')}: ${e}`, 3)
            })
    }

    closeRename()
}

function closePlayer() {
    showPreviewModal.value = false
    isVideo.value = false
}

function openCreate() {
    newFolderName.value = ''
    createError.value = ''
    showCreateModal.value = true

    nextTick(() => createInput.value?.focus())
}

async function createFolder() {
    const name = newFolderName.value.trim()

    if (!name || /[/\\]/.test(name)) {
        createError.value = t('files2.nameInvalid')
        return
    }

    if (mediaStore.folderTree.folders.some((folder) => folder.name === name)) {
        createError.value = t('files2.folderExists', { name })
        return
    }

    showCreateModal.value = false

    const path = `${mediaStore.folderTree.source}/${name}`.replace(/\/[/]+/g, '/')
    lastPath.value = mediaStore.folderTree.source

    await authFetch(`/api/file/${configStore.channels[configStore.i]?.id}/create-folder`, {
        method: 'POST',
        headers: { ...configStore.contentType, ...authStore.authHeader },
        body: JSON.stringify({ source: path }),
    })
        .then(() => {
            indexStore.msgAlert('success', t('files2.folderCreated', { name }), 3)
        })
        .catch((e: string) => {
            indexStore.msgAlert('error', `${t('media.folderError')}: ${e}`, 3)
        })

    mediaStore.getTree(lastPath.value)
    newFolderName.value = ''
}

/*
    Upload dialog
*/
function isAccepted(file: File): boolean {
    if (!extensionsArr.value.length) {
        return true
    }

    return extensionsArr.value.some((suffix) => file.name.toLowerCase().endsWith(suffix))
}

function addFiles(files: File[]) {
    const accepted: File[] = []
    const rejected: string[] = []

    for (const file of files) {
        if (!isAccepted(file)) {
            rejected.push(file.name)
        } else if (!inputFiles.value.some((known) => known.name === file.name)) {
            accepted.push(file)
        }
    }

    inputFiles.value = [...inputFiles.value, ...accepted]
    skippedNames.value = rejected
}

function openUploadWith(files: File[]) {
    if (sending.value) {
        return
    }

    if (!showUploadModal.value) {
        resetUploadDialog()
    }

    showUploadModal.value = true
    addFiles(files)
}

function openUploadDialog() {
    resetUploadDialog()
    showUploadModal.value = true
}

function resetUploadDialog() {
    inputFiles.value = []
    skippedNames.value = []
    conflictNames.value = []
    uploadError.value = ''
    uploadedCount.value = 0
    uploadStage.value = 'choose'
    resetUploadState()

    if (fileInput.value) {
        fileInput.value.value = ''
    }
}

function closeUploadDialog() {
    if (sending.value) {
        return
    }

    showUploadModal.value = false
    resetUploadDialog()
}

function onFileChange(evt: Event) {
    const files = (evt.target as HTMLInputElement).files

    if (files?.length) {
        addFiles(Array.from(files))
    }

    if (fileInput.value) {
        fileInput.value.value = ''
    }
}

function onUploadDrop(event: DragEvent) {
    uploadDropActive.value = false

    if (event.dataTransfer?.files.length) {
        addFiles(Array.from(event.dataTransfer.files))
    }
}

function removeChosenFile(name: string) {
    inputFiles.value = inputFiles.value.filter((file) => file.name !== name)
}

function startUpload() {
    if (!inputFiles.value.length || sending.value) {
        return
    }

    const existing = new Set(mediaStore.folderTree.files.map((file) => file.name))
    conflictNames.value = inputFiles.value.filter((file) => existing.has(file.name)).map((file) => file.name)

    if (conflictNames.value.length) {
        showOverwriteModal.value = true
        return
    }

    runUpload(inputFiles.value)
}

async function removeExistingFiles(names: string[]) {
    for (const name of names) {
        await authFetch<void>(`/api/file/${configStore.channels[configStore.i]?.id}/remove`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({ source: pathOf(name), recursive: false }),
        })
    }
}

async function answerOverwrite(replace: boolean) {
    showOverwriteModal.value = false

    if (replace) {
        try {
            await removeExistingFiles(conflictNames.value)
        } catch (err: any) {
            uploadStage.value = 'error'
            uploadError.value = err?.message || String(err)
            return
        }

        await runUpload(inputFiles.value)
        return
    }

    // "No" and Escape both keep the old files and send nothing; the operator
    // can drop the repeated names from the list and send again.
}

async function runUpload(filesToUpload: File[]) {
    await authStore.inspectToken()
    lastPath.value = mediaStore.folderTree.source
    uploadStage.value = 'sending'
    uploadError.value = ''
    uploadedCount.value = 0

    try {
        await uploadFilesComposable(filesToUpload, {
            buildRequest: (_file, currentIndex, batchCount) => ({
                url: `/api/file/${configStore.channels[configStore.i]?.id}/upload?path=${encodeURIComponent(
                    mediaStore.crumbs[mediaStore.crumbs.length - 1]?.path ?? '',
                )}`,
                headers: authStore.authHeader,
                method: 'PUT',
                currentIndex,
                batchCount,
            }),
            onFileComplete: () => {
                uploadedCount.value++
            },
        })

        uploadStage.value = 'done'
        currentProgress.value = 100
    } catch (err: any) {
        uploadStage.value = 'error'
        uploadError.value = err?.message || t('media.uploadError')
    }

    mediaStore.getTree(lastPath.value)
}
</script>
<template>
    <div
        class="relative w-full px-3 sm:px-6 py-5 max-w-6xl mx-auto"
        @dragenter="onPageDragEnter"
        @dragleave="onPageDragLeave"
        @dragover.prevent
        @drop.prevent="onPageDrop"
    >
        <PageHeader :title="t('nav.files')" :subtitle="t('files2.subtitle')">
            <template #actions>
                <button class="btn btn-primary" @click="openUploadDialog">
                    <i class="bi bi-cloud-arrow-up-fill" />
                    {{ t('files2.upload') }}
                </button>
                <button class="btn" @click="openCreate">
                    <i class="bi bi-folder-plus" />
                    {{ t('files2.newFolder') }}
                </button>
            </template>
        </PageHeader>

        <!-- Where am I: folder trail -->
        <nav class="flex flex-wrap items-center gap-2 mb-3" :aria-label="t('files2.trail')">
            <button v-if="canGoUp" class="btn btn-sm" @click="goUp">
                <i class="bi bi-arrow-up-left" />
                {{ t('files2.up') }}
            </button>
            <ol class="flex flex-wrap items-center gap-1 text-base min-w-0">
                <li
                    v-for="(crumb, index) in mediaStore.crumbs"
                    :key="index"
                    class="flex items-center gap-1 min-w-0"
                    @dragover.prevent="handleDragOver($event, `crumb:${index}`)"
                    @dragleave="handleDragLeave(`crumb:${index}`)"
                    @drop.prevent.stop="handleDrop($event, crumb.path, null)"
                >
                    <i v-if="index > 0" class="bi bi-chevron-right text-xs opacity-50" />
                    <span
                        v-if="index === mediaStore.crumbs.length - 1"
                        class="rounded-lg px-2 py-1 font-bold bg-base-100 shadow-sm truncate"
                        aria-current="page"
                    >
                        <i :class="index === 0 ? 'bi bi-house-door-fill' : 'bi bi-folder2-open'" class="me-1" />
                        {{ index === 0 ? t('files2.root') : crumb.text }}
                    </span>
                    <button
                        v-else
                        class="rounded-lg px-2 py-1 text-primary font-semibold hover:bg-base-100 truncate cursor-pointer"
                        :class="dropTarget === `crumb:${index}` && 'bg-primary/20 ring-2 ring-primary'"
                        @click="mediaStore.getTree(crumb.path)"
                    >
                        <i :class="index === 0 ? 'bi bi-house-door' : 'bi bi-folder'" class="me-1" />
                        {{ index === 0 ? t('files2.root') : crumb.text }}
                    </button>
                </li>
            </ol>
        </nav>

        <!-- Sibling folders: quick jump and drop targets for moving files up -->
        <div
            v-if="mediaStore.folderTree.parent_folders.length > 0 && canGoUp"
            class="flex items-center gap-2 mb-3 text-sm overflow-x-auto pb-1"
        >
            <span class="opacity-70 shrink-0">{{ t('files2.siblings') }}</span>
            <button
                v-for="folder in mediaStore.folderTree.parent_folders"
                :key="folder.uid"
                class="btn btn-sm rounded-full shrink-0"
                :class="[
                    filename(currentSource) === folder.name && 'btn-primary pointer-events-none',
                    dropTarget === `parent:${folder.uid}` && 'btn-primary',
                ]"
                @click="openParentFolder(folder.name)"
                @dragover.prevent="handleDragOver($event, `parent:${folder.uid}`)"
                @dragleave="handleDragLeave(`parent:${folder.uid}`)"
                @drop.prevent.stop="handleDrop($event, folder, true)"
            >
                <i class="bi bi-folder-fill" />
                {{ folder.name }}
            </button>
        </div>

        <section class="relative card bg-base-100 shadow overflow-hidden">
            <div
                class="flex flex-wrap items-center gap-3 px-4 py-3 border-b border-base-300 bg-base-100"
                :class="entryCount === 0 && 'hidden'"
            >
                <div class="text-sm opacity-70 grow">
                    {{ countLabel }}
                </div>
                <label v-if="entryCount > 6" class="input input-sm w-full sm:w-64">
                    <i class="bi bi-search opacity-60" />
                    <input v-model="filterText" type="search" :placeholder="t('files2.search')" />
                </label>
                <span class="hidden md:inline text-sm opacity-60">
                    <i class="bi bi-hand-index-thumb" />
                    {{ t('files2.dragHint') }}
                    <HelpTip :text="t('files2.dragHelp')" />
                </span>
            </div>

            <div v-if="mediaStore.isLoading" class="absolute inset-0 z-10 flex justify-center pt-16 bg-base-100/70">
                <span class="loading loading-spinner loading-lg text-primary" />
            </div>

            <EmptyState
                v-if="!mediaStore.isLoading && isEmptyFolder"
                icon="bi-folder2-open"
                :title="t('files2.emptyTitle')"
                :text="t('files2.emptyText')"
            >
                <button class="btn btn-primary" @click="openUploadDialog">
                    <i class="bi bi-cloud-arrow-up-fill" />
                    {{ t('files2.upload') }}
                </button>
                <button class="btn" @click="openCreate">
                    <i class="bi bi-folder-plus" />
                    {{ t('files2.newFolder') }}
                </button>
            </EmptyState>

            <p
                v-else-if="!isEmptyFolder && shownFolders.length + shownFiles.length === 0"
                class="p-6 text-center opacity-70"
            >
                {{ t('files2.noMatch', { text: filterText }) }}
            </p>

            <ul v-if="shownFolders.length + shownFiles.length > 0" class="divide-y divide-base-300">
                <li
                    v-for="folder in shownFolders"
                    :key="folder.uid"
                    class="flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-3 transition-colors"
                    :class="
                        dropTarget === `folder:${folder.uid}`
                            ? 'bg-primary/15 ring-2 ring-inset ring-primary'
                            : 'hover:bg-base-200/60'
                    "
                    @dragover.prevent="handleDragOver($event, `folder:${folder.uid}`)"
                    @dragleave="handleDragLeave(`folder:${folder.uid}`)"
                    @drop.prevent.stop="handleDrop($event, folder, false)"
                >
                    <button
                        class="flex items-center gap-3 grow basis-48 min-w-0 text-left cursor-pointer"
                        :title="t('files2.openFolder', { name: folder.name })"
                        @click="openFolder(folder.name)"
                    >
                        <span
                            class="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-warning/20 text-warning text-2xl"
                        >
                            <i
                                :class="
                                    dropTarget === `folder:${folder.uid}` ? 'bi bi-folder2-open' : 'bi bi-folder-fill'
                                "
                            />
                        </span>
                        <span class="min-w-0">
                            <span class="block font-semibold break-words">{{ folder.name }}</span>
                            <span class="block text-sm opacity-70">
                                {{ t('files2.typeFolder') }} ·
                                <span class="text-primary">{{ t('files2.open') }}</span>
                            </span>
                        </span>
                    </button>
                    <div class="flex gap-1 w-full sm:w-auto">
                        <button class="btn btn-sm btn-ghost grow sm:grow-0" @click="setRenameValues(folder.name, true)">
                            <i class="bi bi-pencil-square" />
                            {{ t('files2.rename') }}
                        </button>
                        <button class="btn btn-sm btn-ghost text-error grow sm:grow-0" @click="askDelete(folder.name)">
                            <i class="bi bi-trash3" />
                            {{ t('files2.delete') }}
                        </button>
                    </div>
                </li>

                <li
                    v-for="element in shownFiles"
                    :key="element.name"
                    class="flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-3 hover:bg-base-200/60"
                    draggable="true"
                    @dragstart="handleDragStart($event, element)"
                >
                    <div class="flex items-center gap-3 grow basis-48 min-w-0">
                        <span
                            class="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl text-2xl"
                            :class="typeIcon(element.name)"
                        />
                        <div class="min-w-0">
                            <div class="font-semibold break-words">{{ element.name }}</div>
                            <div class="text-sm opacity-70">
                                {{ typeLabel(element.name) }}
                                <template v-if="mediaType(element.name) !== 'image'">
                                    ·
                                    <i class="bi bi-clock" />
                                    {{ humanDuration(element.duration) }}
                                </template>
                            </div>
                        </div>
                    </div>
                    <div class="flex gap-1 w-full sm:w-auto">
                        <button
                            class="btn btn-sm btn-primary btn-soft grow sm:grow-0"
                            @click="openPreview(element.name)"
                        >
                            <i class="bi bi-play-fill" />
                            {{ t('files2.view') }}
                        </button>
                        <button
                            class="btn btn-sm btn-ghost grow sm:grow-0"
                            @click="setRenameValues(element.name, false)"
                        >
                            <i class="bi bi-pencil-square" />
                            {{ t('files2.rename') }}
                        </button>
                        <button class="btn btn-sm btn-ghost text-error grow sm:grow-0" @click="askDelete(element.name)">
                            <i class="bi bi-trash3" />
                            {{ t('files2.delete') }}
                        </button>
                    </div>
                </li>
            </ul>
        </section>

        <!-- Overlay shown while the operator drags files from the computer over the page -->
        <div
            v-if="isFileDragging"
            class="fixed inset-0 z-40 flex items-center justify-center bg-primary/25 p-6 pointer-events-none"
        >
            <div
                class="rounded-3xl border-4 border-dashed border-primary bg-base-100 px-10 py-12 text-center shadow-2xl max-w-md"
            >
                <i class="bi bi-cloud-arrow-up-fill text-6xl text-primary" />
                <div class="mt-2 text-2xl font-bold">{{ t('files2.dropTitle') }}</div>
                <p class="mt-1 opacity-70">{{ t('files2.dropText') }}</p>
            </div>
        </div>

        <!-- Upload dialog -->
        <div
            v-if="showUploadModal"
            class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-3 sm:p-4"
            role="dialog"
            aria-modal="true"
            @keydown.esc="closeUploadDialog"
        >
            <div class="w-full max-w-2xl max-h-[92vh] overflow-y-auto rounded-2xl bg-base-100 p-5 sm:p-6 shadow-2xl">
                <div class="flex items-start gap-3">
                    <h2 class="grow text-xl font-bold">{{ t('files2.uploadTitle') }}</h2>
                    <button
                        class="btn btn-sm btn-circle btn-ghost"
                        :disabled="sending"
                        :aria-label="t('ui.close')"
                        @click="closeUploadDialog"
                    >
                        <i class="bi bi-x-lg" />
                    </button>
                </div>
                <p class="mt-1 text-sm opacity-70">
                    {{ t('files2.uploadInto') }}
                    <strong>{{
                        mediaStore.crumbs.length > 1
                            ? mediaStore.crumbs[mediaStore.crumbs.length - 1]?.text
                            : t('files2.root')
                    }}</strong>
                </p>

                <!-- 1. Choose -->
                <template v-if="uploadStage === 'choose'">
                    <div
                        class="mt-4 flex flex-col items-center gap-2 rounded-2xl border-2 border-dashed px-4 py-8 text-center transition-colors"
                        :class="
                            uploadDropActive ? 'border-primary bg-primary/10' : 'border-base-content/30 bg-base-200/60'
                        "
                        @dragover.prevent.stop="uploadDropActive = true"
                        @dragleave.stop="uploadDropActive = false"
                        @drop.prevent.stop="onUploadDrop"
                    >
                        <i class="bi bi-cloud-arrow-up text-5xl text-primary" />
                        <div class="text-lg font-semibold">{{ t('files2.dropHere') }}</div>
                        <div class="opacity-70">{{ t('files2.or') }}</div>
                        <button class="btn btn-primary" @click="fileInput?.click()">
                            <i class="bi bi-folder2-open" />
                            {{ t('files2.pickFiles') }}
                        </button>
                        <input
                            ref="fileInput"
                            type="file"
                            class="hidden"
                            :accept="extensions"
                            multiple
                            @change="onFileChange"
                        />
                        <p class="mt-1 text-sm opacity-60">{{ t('files2.accepted', { list: extensions }) }}</p>
                    </div>

                    <div v-if="skippedNames.length" class="alert alert-warning mt-3 text-sm">
                        <i class="bi bi-exclamation-triangle-fill" />
                        <span>{{ t('files2.skipped', { names: skippedNames.join(', ') }) }}</span>
                    </div>

                    <div v-if="inputFiles.length" class="mt-4">
                        <div class="text-sm font-semibold mb-1">
                            {{ t('files2.chosen', { count: inputFiles.length }, inputFiles.length) }}
                            <span class="font-normal opacity-70">({{ humanSize(totalUploadSize, locale) }})</span>
                        </div>
                        <ul class="max-h-52 overflow-y-auto divide-y divide-base-300 rounded-lg border border-base-300">
                            <li v-for="file in inputFiles" :key="file.name" class="flex items-center gap-2 px-3 py-2">
                                <i
                                    class="bi"
                                    :class="mediaType(file.name) === 'audio' ? 'bi-music-note-beamed' : 'bi-film'"
                                />
                                <span class="grow min-w-0 break-words">{{ file.name }}</span>
                                <span class="text-sm opacity-70 whitespace-nowrap">{{
                                    humanSize(file.size, locale)
                                }}</span>
                                <button
                                    class="btn btn-xs btn-circle btn-ghost"
                                    :aria-label="t('files2.removeFromList')"
                                    :title="t('files2.removeFromList')"
                                    @click="removeChosenFile(file.name)"
                                >
                                    <i class="bi bi-x-lg" />
                                </button>
                            </li>
                        </ul>
                    </div>

                    <div class="mt-5 flex flex-wrap justify-end gap-2">
                        <button class="btn" @click="closeUploadDialog">{{ t('ui.cancel') }}</button>
                        <button class="btn btn-primary" :disabled="!inputFiles.length" @click="startUpload">
                            <i class="bi bi-cloud-arrow-up-fill" />
                            {{ t('files2.sendCount', { count: inputFiles.length }, inputFiles.length) }}
                        </button>
                    </div>
                </template>

                <!-- 2. Sending -->
                <template v-else-if="uploadStage === 'sending'">
                    <div class="mt-6 text-center">
                        <div class="text-lg font-semibold">
                            {{ t('files2.sending', { current: currentNumber, total: inputFiles.length }) }}
                        </div>
                        <div class="mt-1 break-words opacity-80">{{ uploadTask }}</div>
                        <div class="mt-4 flex items-center gap-3">
                            <progress class="progress progress-primary w-full h-4" :value="percent" max="100" />
                            <span class="w-12 text-end font-bold tabular-nums">{{ percent }}%</span>
                        </div>
                        <p class="mt-4 text-sm opacity-70">
                            <i class="bi bi-info-circle" />
                            {{ t('files2.keepOpen') }}
                        </p>
                    </div>
                </template>

                <!-- 3. Done -->
                <template v-else-if="uploadStage === 'done'">
                    <div class="mt-6 text-center">
                        <i class="bi bi-check-circle-fill text-6xl text-success" />
                        <div class="mt-2 text-xl font-bold">{{ t('files2.doneTitle') }}</div>
                        <p class="opacity-70">{{ t('files2.doneText', { count: uploadedCount }, uploadedCount) }}</p>
                    </div>
                    <div class="mt-6 flex flex-wrap justify-end gap-2">
                        <button class="btn" @click="resetUploadDialog">{{ t('files2.sendMore') }}</button>
                        <button class="btn btn-primary" @click="closeUploadDialog">{{ t('ui.close') }}</button>
                    </div>
                </template>

                <!-- 4. Error -->
                <template v-else>
                    <div class="alert alert-error mt-5">
                        <i class="bi bi-x-octagon-fill" />
                        <div>
                            <div class="font-bold">{{ t('files2.errorTitle') }}</div>
                            <div class="text-sm break-words">{{ uploadError }}</div>
                            <div v-if="uploadedCount" class="text-sm mt-1">
                                {{ t('files2.errorPartial', { count: uploadedCount }, uploadedCount) }}
                            </div>
                        </div>
                    </div>
                    <div class="mt-6 flex flex-wrap justify-end gap-2">
                        <button class="btn" @click="closeUploadDialog">{{ t('ui.close') }}</button>
                        <button class="btn btn-primary" @click="resetUploadDialog">{{ t('files2.tryAgain') }}</button>
                    </div>
                </template>
            </div>
        </div>

        <!-- Overwrite confirmation -->
        <ConfirmModal
            :show="showOverwriteModal"
            danger
            :title="t('files2.overwriteTitle', {}, conflictNames.length)"
            :confirm-label="t('files2.overwriteYes')"
            :cancel-label="t('files2.overwriteNo')"
            @confirm="answerOverwrite(true)"
            @cancel="showOverwriteModal = false"
        >
            <p class="mt-2 opacity-80">{{ t('files2.overwriteText') }}</p>
            <ul class="mt-2 max-h-32 overflow-y-auto rounded-lg bg-base-200 p-2 text-sm font-semibold">
                <li v-for="name in conflictNames" :key="name" class="break-words">{{ name }}</li>
            </ul>
            <p class="mt-2 text-sm opacity-80">{{ t('files2.overwriteWarn') }}</p>
        </ConfirmModal>

        <!-- Delete confirmation -->
        <ConfirmModal
            :show="showDeleteModal"
            danger
            :title="deleteIsFile ? t('files2.deleteFileTitle') : t('files2.deleteFolderTitle')"
            :confirm-label="deleteIsFile ? t('files2.deleteFileYes') : t('files2.deleteFolderYes')"
            :cancel-label="t('files2.deleteNo')"
            @confirm="deleteFileOrFolder(true)"
            @cancel="deleteFileOrFolder(false)"
        >
            <p class="mt-2 rounded-lg bg-base-200 p-2 font-semibold break-words">{{ dir_file(deleteName).file }}</p>
            <p class="mt-3 opacity-80">
                {{ deleteIsFile ? t('files2.deleteFileText') : t('files2.deleteFolderText') }}
            </p>
            <p class="mt-2 font-bold text-error">{{ t('files2.noUndo') }}</p>
            <label v-if="!deleteIsFile" class="mt-3 flex items-start gap-2 cursor-pointer">
                <input v-model="recursive" type="checkbox" class="checkbox checkbox-error mt-0.5" />
                <span>{{ t('files2.deleteRecursive') }}</span>
            </label>
        </ConfirmModal>

        <!-- Rename -->
        <ConfirmModal
            :show="showRenameModal"
            :title="renameIsFolder ? t('files2.renameFolderTitle') : t('files2.renameFileTitle')"
            :confirm-label="t('files2.renameSave')"
            @confirm="renameFile"
            @cancel="closeRename"
        >
            <label class="mt-3 block">
                <span class="text-sm font-semibold">{{ t('files2.newName') }}</span>
                <input
                    ref="renameInput"
                    v-model="renameNewName"
                    type="text"
                    class="input w-full mt-1"
                    @keydown.enter.prevent="renameFile"
                />
            </label>
            <p v-if="!renameIsFolder" class="mt-2 text-sm opacity-70">{{ t('files2.keepExtension') }}</p>
            <p v-if="renameError" class="mt-2 text-sm font-semibold text-error">{{ renameError }}</p>
        </ConfirmModal>

        <!-- New folder -->
        <ConfirmModal
            :show="showCreateModal"
            :title="t('files2.newFolder')"
            :confirm-label="t('files2.createFolderYes')"
            @confirm="createFolder"
            @cancel="showCreateModal = false"
        >
            <label class="mt-3 block">
                <span class="text-sm font-semibold">{{ t('files2.folderNameLabel') }}</span>
                <input
                    ref="createInput"
                    v-model="newFolderName"
                    type="text"
                    class="input w-full mt-1"
                    :placeholder="t('files2.folderNamePlaceholder')"
                    @keydown.enter.prevent="createFolder"
                />
            </label>
            <p class="mt-2 text-sm opacity-70">{{ t('files2.folderNameHelp') }}</p>
            <p v-if="createError" class="mt-2 text-sm font-semibold text-error">{{ createError }}</p>
        </ConfirmModal>

        <!-- Preview -->
        <GenericModal
            :show="showPreviewModal"
            :title="`${t('files2.previewTitle')}: ${previewName}`"
            :modal-action="closePlayer"
            :hide-buttons="true"
        >
            <div class="w-5xl max-w-full aspect-video">
                <VideoPlayer v-if="isVideo && previewOpt" reference="previewPlayer" :options="previewOpt" />
                <img v-else :src="previewUrl" class="img-fluid" :alt="previewName" />
            </div>
        </GenericModal>
    </div>
</template>
