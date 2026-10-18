<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, useTemplateRef, watch } from 'vue'
import { RouterLink, useRoute, type RouteLocationRaw } from 'vue-router'

export interface ScrollTab {
    name: string
    label: string
    icon?: string
    /** A dot warns that the tab has changes that were not saved. */
    dirty?: boolean
}

const props = defineProps<{ tabs: ScrollTab[]; query?: Record<string, unknown>; dirtyLabel?: string }>()

const route = useRoute()
const list = useTemplateRef<HTMLElement>('list')
const more = ref({ left: false, right: false })

function target(tab: ScrollTab): RouteLocationRaw {
    return { name: tab.name, query: props.query as Record<string, string> }
}

function measure() {
    const element = list.value

    if (!element) {
        return
    }

    more.value = {
        left: element.scrollLeft > 4,
        right: element.scrollLeft + element.clientWidth < element.scrollWidth - 4,
    }
}

// On the phone the row scrolls sideways: the tab in use is brought to the middle, so the
// person always sees where they are and that there is more on the sides.
async function centerActive() {
    await nextTick()
    list.value?.querySelector('[aria-current="page"]')?.scrollIntoView({ inline: 'center', block: 'nearest' })
    measure()
}

let observer: ResizeObserver | undefined

onMounted(() => {
    void centerActive()

    if (list.value) {
        observer = new ResizeObserver(measure)
        observer.observe(list.value)
    }
})

onBeforeUnmount(() => observer?.disconnect())
watch(() => [route.name, props.tabs.length], centerActive)
</script>
<template>
    <div class="relative">
        <nav
            ref="list"
            role="tablist"
            class="tabs tabs-border flex-nowrap overflow-x-auto border-b border-base-300 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
            style="scroll-snap-type: x proximity; -webkit-overflow-scrolling: touch"
            @scroll.passive="measure"
        >
            <RouterLink
                v-for="tab in tabs"
                :key="tab.name"
                role="tab"
                :to="target(tab)"
                class="tab h-12 shrink-0 snap-center gap-2 whitespace-nowrap px-4 text-base"
                :class="route.name === tab.name ? 'tab-active font-semibold' : 'text-base-content/75'"
            >
                <i v-if="tab.icon" class="bi" :class="tab.icon" />
                {{ tab.label }}
                <span
                    v-if="tab.dirty"
                    class="h-2.5 w-2.5 rounded-full bg-warning"
                    role="img"
                    :title="dirtyLabel"
                    :aria-label="dirtyLabel"
                />
            </RouterLink>
        </nav>
        <!-- Pistas de que há mais abas: sombra e seta nas pontas que ainda têm o que rolar. -->
        <div
            v-if="more.left"
            class="pointer-events-none absolute inset-y-0 left-0 flex w-10 items-center bg-linear-to-r from-base-200 to-transparent md:hidden"
            aria-hidden="true"
        >
            <i class="bi bi-chevron-left text-sm opacity-60" />
        </div>
        <div
            v-if="more.right"
            class="pointer-events-none absolute inset-y-0 right-0 flex w-10 items-center justify-end bg-linear-to-l from-base-200 to-transparent md:hidden"
            aria-hidden="true"
        >
            <i class="bi bi-chevron-right text-sm opacity-60" />
        </div>
    </div>
</template>
