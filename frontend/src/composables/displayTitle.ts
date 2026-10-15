import { i18n } from '@/i18n'

const PLANNER_TITLES: Record<string, string> = {
    'Break filler': 'plannerTitles.breakFiller',
    'Day filler': 'plannerTitles.dayFiller',
}

/**
 * The break planner stores its filler titles in English; show them in the
 * interface language. Any other title is returned unchanged.
 */
export function displayTitle<T extends string | null | undefined>(title: T): T | string {
    const key = title ? PLANNER_TITLES[title] : undefined

    return key ? i18n.t(key) : title
}
