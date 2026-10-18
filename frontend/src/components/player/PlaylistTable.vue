<script setup lang="ts">
import dayjs from 'dayjs'
import customParseFormat from 'dayjs/plugin/customParseFormat.js'
import LocalizedFormat from 'dayjs/plugin/localizedFormat.js'
import timezone from 'dayjs/plugin/timezone.js'
import utc from 'dayjs/plugin/utc.js'
import 'dayjs/locale/de'
import 'dayjs/locale/en'
import 'dayjs/locale/es'
import 'dayjs/locale/pt-br'
import 'dayjs/locale/ru'

import VirtualList from 'vue-virtual-sortable'

import { ref, nextTick, onMounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useWindowSize, until } from '@vueuse/core'
import { storeToRefs } from 'pinia'

dayjs.extend(customParseFormat)
dayjs.extend(LocalizedFormat)
dayjs.extend(timezone)
dayjs.extend(utc)

import { stringFormatter, playlistOperations } from '@/composables/helper'
import ActionSheet from '@/components/ui/ActionSheet.vue'
import EmptyState from '@/components/ui/EmptyState.vue'
import { displayTitle } from '@/composables/displayTitle'
import { useConfig } from '@/stores/config'
import { useMedia } from '@/stores/media'
import { usePlaylist } from '@/stores/playlist'

const { t } = useI18n()
const { width } = useWindowSize({ initialWidth: 800 })
const configStore = useConfig()
const mediaStore = useMedia()
const playlistStore = usePlaylist()
const { secToHMS, filename, secondsToTime } = stringFormatter()
const { processPlaylist, genUID } = playlistOperations()

const playlistContainer = ref()
const sortContainer = ref()
const todayDate = ref(dayjs().tz(configStore.timezone).format('YYYY-MM-DD'))
const { i } = storeToRefs(useConfig())
const { currentIndex, listDate, playoutIsRunning, scrollToItem } = storeToRefs(usePlaylist())

const dragGroup = ref({ name: 'dragGroup', pull: true, put: true })

defineProps({
    editItem: {
        type: Function,
        default() {
            return ''
        },
    },
    preview: {
        type: Function,
        default() {
            return ''
        },
    },
    /** Show the columns that only people who edit the cuts need: start, end, commercial flag. */
    details: {
        type: Boolean,
        default: false,
    },
})

onMounted(() => {
    setTimeout(() => {
        getPlaylist()
    }, 150)
})

watch([listDate, i], () => {
    setTimeout(() => {
        getPlaylist()
    }, 800)
})

watch([playoutIsRunning, scrollToItem], () => {
    if (playoutIsRunning.value || scrollToItem.value) {
        setTimeout(() => {
            scrollTo(currentIndex.value)

            scrollToItem.value = false
        }, 400)
    }
})

defineExpose({
    getPlaylist,
})

function scrollTo(index: number) {
    sortContainer.value.scrollToIndex(index)
}

async function getPlaylist() {
    playlistStore.isLoading = true
    await playlistStore.getPlaylist(listDate.value)
    playlistStore.isLoading = false

    if (listDate.value === todayDate.value) {
        await until(currentIndex).toMatch((v) => v > 0, { timeout: 1500 })
        scrollTo(currentIndex.value)
    } else {
        scrollTo(0)
    }
}

function addBG(item: any) {
    if (item.event?.target) {
        item.event?.target.classList.add('!bg-fuchsia-900/30')
    } else {
        item.classList?.add('!bg-fuchsia-900/30')
    }
}

function removeBG(item: any) {
    setTimeout(() => {
        item.classList?.remove('!bg-fuchsia-900/30')
    }, 1000)
}

function dropItem(event: any) {
    const nIndex = event.newIndex

    if (event.event.from.id === 'mediaList') {
        const media = event.item
        const storagePath = configStore.channels[configStore.i]?.storage
        const sourcePath = `${storagePath}/${mediaStore.folderTree.source}/${media.name}`.replace(/\/[/]+/g, '/')
        const uid = genUID()

        const mediaObject = {
            uid,
            begin: 0,
            source: sourcePath,
            in: 0,
            out: media.duration || 10,
            duration: media.duration || 10,
            ad: false,
        }

        playlistStore.playlist[nIndex] = mediaObject

        nextTick(() => {
            const newNode = document.getElementById(uid)
            addBG(newNode)
            removeBG(newNode)
        })
    }

    processPlaylist(listDate.value, playlistStore.playlist, false)

    nextTick(() => {
        removeBG(event.event.node)

        if (nIndex > playlistStore.playlist.length - 4) {
            sortContainer.value.scrollToBottom()
        }
    })
}

// No celular não há como arrastar com o dedo: subir e descer são botões, na folha de ações do item.
const sheetIndex = ref<number | null>(null)

function moveItem(index: number, delta: number) {
    const target = index + delta

    if (target < 0 || target >= playlistStore.playlist.length) {
        return
    }

    const [moved] = playlistStore.playlist.splice(index, 1)

    playlistStore.playlist.splice(target, 0, moved!)
    processPlaylist(listDate.value, playlistStore.playlist, false)
}

function deletePlaylistItem(index: number) {
    playlistStore.playlist.splice(index, 1)

    processPlaylist(listDate.value, playlistStore.playlist, false)
}
</script>
<style>
#sort-container .timeHidden {
    display: none !important;
}

/*
    format dragging elements
*/
.media-placeholder,
.sortable-ghost {
    background-color: #701a754b !important;
    min-height: 37px !important;
    height: 37px !important;
}

#playlist-container .media-placeholder div:nth-child(1)::after {
    content: '00:00:00';
    padding-left: 4px;
}

#playlist-container .media-placeholder div:nth-child(1) i {
    display: none;
}

#playlist-container .media-placeholder div:nth-child(4),
#playlist-container .media-placeholder div:nth-child(5),
#playlist-container .media-placeholder div:nth-child(6) {
    padding-left: 18px;
}

#playlist-container .media-placeholder {
    grid-template-columns: 85px auto 85px 85px;
}

@media (max-width: 875px) {
    #playlist-container .media-placeholder div:nth-child(4) {
        display: none;
    }
}

@media (min-width: 876px) {
    #playlist-container .media-placeholder {
        grid-template-columns: 85px auto 85px 85px 85px 85px 85px 85px 85px;
    }

    #playlist-container .media-placeholder div {
        display: block;
    }
}
</style>
<template>
    <div class="relative w-full h-full bg-base-300! rounded-e">
        <div v-if="playlistStore.isLoading" class="w-full h-full absolute z-2 flex justify-center bg-base-100/70">
            <span class="loading loading-spinner loading-lg" />
        </div>

        <div
            class="grid bg-base-100 border-b border-base-content/30 rounded-tr-lg py-2 text-sm font-bold text-base-content/70"
            :class="
                details
                    ? 'grid-cols-[76px_minmax(0,1fr)_44px_44px_44px] 2md:grid-cols-[85px_auto_85px_85px_85px_85px_85px_85px_85px]'
                    : 'grid-cols-[76px_minmax(0,1fr)_44px_44px_44px] 2md:grid-cols-[85px_auto_85px_95px_85px_85px]'
            "
        >
            <div v-if="!configStore.playout.playlist.infinit" class="px-2 2md:px-3">
                {{ t('player2.col.time') }}
            </div>
            <div>
                {{ t('player2.col.program') }}
            </div>
            <div class="px-2 text-center">
                {{ t('player2.col.see') }}
            </div>
            <div class="px-2 hidden 2md:block text-center">
                {{ t('player.duration') }}
            </div>
            <div v-if="details" class="px-2 hidden 2md:block text-center">
                {{ t('player.in') }}
            </div>
            <div v-if="details" class="px-2 hidden 2md:block text-center">
                {{ t('player.out') }}
            </div>
            <div v-if="details" class="px-2 hidden 2md:block text-center">
                {{ t('player2.col.ad') }}
            </div>
            <div class="px-2 text-center">
                {{ t('player2.col.edit') }}
            </div>
            <div class="px-2 hidden 2md:block text-center">
                {{ t('player2.col.delete') }}
            </div>
            <div class="2md:hidden" aria-hidden="true" />
        </div>

        <div id="playlist-container" ref="playlistContainer" class="h-[calc(100%-35px)]">
            <VirtualList
                id="sort-container"
                ref="sortContainer"
                v-model="playlistStore.playlist"
                :group="dragGroup"
                class="h-full"
                :handle="'.handle'"
                :data-key="'uid'"
                chosen-class="cursor-grabbing"
                ghost-class="sortable-ghost"
                placeholder-class="sortable-ghost"
                wrap-tag="ul"
                wrap-class="relative list-none text-sm"
                :tableMode="false"
                :animation="100"
                @drop="dropItem"
                @drag="addBG"
            >
                <template #header>
                    <div v-if="playlistStore.playlist.length === 0" class="is-empty relative">
                        <EmptyState
                            icon="bi-calendar-x"
                            :title="t('player2.empty.title')"
                            :text="t('player2.empty.text')"
                        >
                            <RouterLink
                                :to="{ name: 'commercial-today', query: { date: listDate } }"
                                class="btn btn-primary btn-sm"
                            >
                                {{ t('player2.empty.build') }}
                            </RouterLink>
                        </EmptyState>
                    </div>
                </template>
                <template #item="{ item, index }">
                    <li
                        :id="item.uid"
                        :key="item.uid"
                        class="grid odd:bg-base-200 border-b border-base-content/20 duration-500 transition-colors py-2"
                        :class="{
                            'grid-cols-[76px_minmax(0,1fr)_44px_44px_44px] 2md:grid-cols-[85px_auto_85px_85px_85px_85px_85px_85px_85px]':
                                details,
                            'grid-cols-[76px_minmax(0,1fr)_44px_44px_44px] 2md:grid-cols-[85px_auto_85px_95px_85px_85px]':
                                !details,
                            'bg-lime-500/30!':
                                playlistStore.playoutIsRunning && listDate === todayDate && index === currentIndex,
                            'bg-amber-600/40!': item.overtime,
                            'text-base-content/60': item.ad,
                        }"
                    >
                        <div v-if="!configStore.playout.playlist.infinit" class="px-2 2md:px-3 text-left">
                            {{ secondsToTime(item.begin) }}
                        </div>
                        <div
                            class="flex items-center gap-1 text-left truncate"
                            :class="{ 'handle cursor-grab': width > 768 }"
                        >
                            <span
                                v-if="
                                    playlistStore.playoutIsRunning && listDate === todayDate && index === currentIndex
                                "
                                class="badge badge-success badge-sm shrink-0"
                            >
                                {{ t('player2.col.now') }}
                            </span>
                            <span v-if="item.ad" class="badge badge-ghost badge-sm shrink-0">
                                {{ t('player2.col.spot') }}
                            </span>
                            <span class="truncate">{{ displayTitle(item.title) || filename(item.source) }}</span>
                            <i
                                v-if="item.audio"
                                class="bi-music-note-beamed shrink-0"
                                :title="`${t('player.audio')}: ${filename(item.audio)}`"
                            />
                        </div>
                        <div class="text-center hover:text-base-content/70">
                            <button
                                class="btn btn-ghost btn-square btn-xs"
                                :aria-label="t('player2.col.see')"
                                @click="preview(item.source)"
                            >
                                <i class="bi-play-fill text-base" />
                            </button>
                        </div>
                        <div class="text-center hidden 2md:block">{{ secToHMS(item.duration) }}</div>
                        <div v-if="details" class="text-center hidden 2md:block">
                            {{ secToHMS(item.in) }}
                        </div>
                        <div v-if="details" class="text-center hidden 2md:block">
                            {{ secToHMS(item.out) }}
                        </div>
                        <div v-if="details" class="text-center hidden 2md:block leading-3">
                            <input v-model="item.ad" class="checkbox checkbox-xs rounded" type="checkbox" />
                        </div>
                        <div class="text-center hover:text-base-content/70">
                            <button
                                class="btn btn-ghost btn-square btn-xs"
                                :aria-label="t('player2.col.edit')"
                                @click="editItem(index)"
                            >
                                <i class="bi-pencil-square text-base" />
                            </button>
                        </div>
                        <div
                            class="text-center hidden 2md:block justify-center text-base-content/80 hover:text-base-content/60"
                        >
                            <button
                                class="btn btn-ghost btn-square btn-xs"
                                :aria-label="t('player2.col.delete')"
                                @click="deletePlaylistItem(index)"
                            >
                                <i class="bi-x-circle-fill text-base" />
                            </button>
                        </div>
                        <div class="text-center 2md:hidden">
                            <button
                                class="btn btn-ghost btn-square btn-xs"
                                :aria-label="t('player2.item.actions')"
                                @click="sheetIndex = index"
                            >
                                <i class="bi-three-dots-vertical text-base" />
                            </button>
                        </div>
                    </li>
                </template>
            </VirtualList>
        </div>

        <ActionSheet
            :show="sheetIndex !== null"
            :title="
                sheetIndex !== null
                    ? displayTitle(playlistStore.playlist[sheetIndex]?.title) ||
                      filename(playlistStore.playlist[sheetIndex]?.source ?? '')
                    : ''
            "
            @close="sheetIndex = null"
        >
            <template v-if="sheetIndex !== null">
                <button
                    class="btn btn-ghost justify-start"
                    :disabled="sheetIndex === 0"
                    @click="moveItem(sheetIndex, -1)"
                >
                    <i class="bi bi-arrow-up" /> {{ t('player2.item.up') }}
                </button>
                <button
                    class="btn btn-ghost justify-start"
                    :disabled="sheetIndex === playlistStore.playlist.length - 1"
                    @click="moveItem(sheetIndex, 1)"
                >
                    <i class="bi bi-arrow-down" /> {{ t('player2.item.down') }}
                </button>
                <button class="btn btn-ghost justify-start text-error" @click="deletePlaylistItem(sheetIndex)">
                    <i class="bi bi-x-circle" /> {{ t('player2.item.remove') }}
                </button>
            </template>
        </ActionSheet>
    </div>
</template>
