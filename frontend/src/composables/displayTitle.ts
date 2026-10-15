import { i18n } from '@/i18n'

/** The planner names its filler items in English; show them in the language of the screen. */
const PLANNER_TITLES: Record<string, string> = {
    'Break filler': 'plannerTitles.breakFiller',
    'Day filler': 'plannerTitles.dayFiller',
    'Gap filler': 'plannerTitles.gapFiller',
}

export function displayTitle(title: string | null | undefined): string {
    if (!title) {
        return ''
    }

    const key = PLANNER_TITLES[title]

    return key ? i18n.t(key) : title
}
