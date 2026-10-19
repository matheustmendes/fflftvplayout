import { authFetch } from '@/composables/authFetch'
import { useConfig } from '@/stores/config'

/** Titles the day planner gives to what it adds to fill time; they are not programs. */
const FILLER_TITLES = new Set(['break filler', 'gap filler', 'day filler'])

/** One program on the air in a day's playlist, with the clock time it starts. */
export interface ProgramOccurrence {
    /** `YYYY-MM-DD`, the playlist day. */
    date: string
    /** `HH:MM` */
    time: string
    title: string
}

function isoDate(date: Date): string {
    const month = String(date.getMonth() + 1).padStart(2, '0')
    const day = String(date.getDate()).padStart(2, '0')

    return `${date.getFullYear()}-${month}-${day}`
}

function clock(seconds: number): string {
    const minutes = Math.floor(seconds / 60) % 1440

    return `${String(Math.floor(minutes / 60)).padStart(2, '0')}:${String(minutes % 60).padStart(2, '0')}`
}

/** What a graphic can be tied to: the programs of the playlists, not the spots. */
export function useProgramList() {
    const configStore = useConfig()

    async function upcoming(days: number): Promise<ProgramOccurrence[]> {
        const channel = configStore.channels[configStore.i]?.id
        const startInSec = configStore.playout.playlist?.startInSec ?? 0

        if (!channel) {
            return []
        }

        const dates = Array.from({ length: days }, (_, index) => {
            const day = new Date()

            day.setDate(day.getDate() + index)

            return isoDate(day)
        })
        const playlists = await Promise.all(
            dates.map(async (date) => {
                try {
                    const data = await authFetch<{ program?: PlaylistItem[] }>(`/api/playlist/${channel}?date=${date}`)

                    return data.program ?? []
                } catch {
                    // A day without a playlist yet simply has no programs to offer.
                    return []
                }
            }),
        )

        return dates.flatMap((date, index) => {
            let begin = startInSec

            return playlists[index].flatMap((item) => {
                const start = begin

                begin += item.out - item.in

                const title = (item.title ?? '').trim()

                return item.ad || !title || FILLER_TITLES.has(title.toLowerCase())
                    ? []
                    : [{ date, time: clock(start), title }]
            })
        })
    }

    return { upcoming }
}
