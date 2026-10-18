/**
 * Keeps the phone screen on while something long runs (an upload from the gallery): when the
 * screen turns off, iPhone and Android pause the page and the upload stops.
 *
 * Returns a function that lets the screen sleep again. Where the browser has no support, or
 * refuses, it does nothing and the upload goes on as usual.
 */
export async function keepScreenAwake(): Promise<() => void> {
    type Sentinel = { release: () => Promise<void> }
    const wakeLock = (navigator as Navigator & { wakeLock?: { request: (type: 'screen') => Promise<Sentinel> } })
        .wakeLock

    if (!wakeLock) {
        return () => undefined
    }

    let sentinel: Sentinel | null = null

    try {
        sentinel = await wakeLock.request('screen')
    } catch {
        return () => undefined
    }

    return () => {
        void sentinel?.release().catch(() => undefined)
        sentinel = null
    }
}
