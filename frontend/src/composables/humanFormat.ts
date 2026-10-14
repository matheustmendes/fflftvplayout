const SIZE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB']

/**
 * Duration in everyday units: "40 s", "6 min 05 s", "1 h 05 min".
 * The unit abbreviations are the same in Portuguese and English.
 */
export function humanDuration(seconds: number | null | undefined): string {
    if (typeof seconds !== 'number' || !Number.isFinite(seconds) || seconds <= 0) {
        return '—'
    }

    const total = Math.round(seconds)
    const hours = Math.floor(total / 3600)
    const minutes = Math.floor((total % 3600) / 60)
    const secs = total % 60

    if (hours > 0) {
        return minutes > 0 ? `${hours} h ${String(minutes).padStart(2, '0')} min` : `${hours} h`
    }

    if (minutes > 0) {
        return secs > 0 ? `${minutes} min ${String(secs).padStart(2, '0')} s` : `${minutes} min`
    }

    return `${secs} s`
}

/**
 * File size in decimal units (MB, GB), the way operating systems show it to users.
 */
export function humanSize(bytes: number | null | undefined, locale = 'pt-BR'): string {
    if (typeof bytes !== 'number' || !Number.isFinite(bytes) || bytes < 0) {
        return '—'
    }

    let value = bytes
    let unit = 0

    while (value >= 1000 && unit < SIZE_UNITS.length - 1) {
        value /= 1000
        unit++
    }

    const digits = unit === 0 || value >= 100 ? 0 : value >= 10 ? 1 : 2
    const formatted = new Intl.NumberFormat(locale, { maximumFractionDigits: digits }).format(value)

    return `${formatted} ${SIZE_UNITS[unit]}`
}
