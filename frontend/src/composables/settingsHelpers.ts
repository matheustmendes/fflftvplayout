import { computed } from 'vue'
import { isEqual } from 'es-toolkit/predicate'

import { useConfig } from '@/stores/config'

// True while the playout form (output, audio and recording tabs) differs from what the server has.
export function usePlayoutDirty() {
    const configStore = useConfig()

    return computed(() => !isEqual(configStore.playout, configStore.playoutSaved))
}

// Texts shown through v-html must never carry raw user input.
export function escapeHtml(value: string): string {
    return value
        .replaceAll('&', '&amp;')
        .replaceAll('<', '&lt;')
        .replaceAll('>', '&gt;')
        .replaceAll('"', '&quot;')
        .replaceAll("'", '&#39;')
}
