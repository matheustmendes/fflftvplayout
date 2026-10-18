import { authFetch } from '@/composables/authFetch'
import { useFileUpload } from '@/composables/useFileUpload'
import { keepScreenAwake } from '@/composables/wakeLock'
import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'

const JSON_HEADERS = { 'content-type': 'application/json;charset=UTF-8' }

/**
 * Upload one file into a folder of the channel storage, creating the folder
 * first. Returns the path of the file relative to the storage.
 */
export function useFolderUpload() {
    const authStore = useAuth()
    const configStore = useConfig()
    const upload = useFileUpload()

    async function uploadToFolder(file: File, folder: string): Promise<string> {
        const channelId = configStore.channels[configStore.i]?.id

        await authFetch(`/api/file/${channelId}/create-folder`, {
            method: 'POST',
            headers: JSON_HEADERS,
            body: JSON.stringify({ source: folder }),
        })
        await authStore.inspectToken()

        // Celular: com a tela apagada o envio pararia.
        const letScreenSleep = await keepScreenAwake()

        try {
            await upload.uploadFiles([file], {
                buildRequest: (_file, currentIndex, batchCount) => ({
                    url: `/api/file/${channelId}/upload?path=${encodeURIComponent(`/${folder}`)}`,
                    headers: authStore.authHeader,
                    method: 'PUT',
                    currentIndex,
                    batchCount,
                }),
            })
        } finally {
            letScreenSleep()
        }

        return `${folder}/${file.name}`
    }

    return {
        uploadToFolder,
        progress: upload.currentProgress,
        uploading: upload.uploading,
        reset: upload.resetUploadState,
    }
}
