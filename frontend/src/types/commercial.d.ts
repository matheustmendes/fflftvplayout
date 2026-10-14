export {}

declare global {
    type CampaignKind = 'campaign' | 'standalone'
    type BreakPosition = 'any' | 'first' | 'last'
    type SpotStatus = 'processing' | 'ready' | 'failed'
    type AudioMapping = 'auto' | 'first_stream' | 'first_two_mono' | 'mix_all'
    type AirResult =
        | 'pending'
        | 'airing'
        | 'played'
        | 'partial'
        | 'skipped'
        | 'interrupted'
        | 'fallback'
        | 'failed'
        | 'not_aired'
        | 'live_missed'

    interface CommercialAdvertiser {
        id: number
        name: string
        contact_name: string
        email: string
        notes: string
        active: boolean
    }

    interface CommercialCampaign {
        id: number
        advertiser_id: number
        name: string
        kind: CampaignKind
        start_date: string | null
        end_date: string | null
        active: boolean
        weight: number
        max_per_day: number | null
        position: BreakPosition
    }

    interface CommercialSpot {
        id: number
        campaign_id: number
        title: string
        file_path: string
        original_filename: string
        duration_sec: number | null
        status: SpotStatus
        progress: number
        error: string
        active: boolean
        created_at: string
    }

    interface CommercialBreakWindow {
        id: number
        weekday: number
        nominal_time: string
        duration_sec: number
        tolerance_sec: number
        enabled: boolean
    }

    interface CommercialWeekItem {
        id: number
        weekday: number
        position: number
        source: string
        title: string
        in_sec: number
        out_sec: number
        duration_sec: number
    }

    interface CommercialSettings {
        audio_mapping: AudioMapping
        loudness_enabled: boolean
        loudness_target_lufs: number
        loudness_true_peak_dbtp: number
    }

    interface CommercialReportSettings {
        station_name: string
        station_details: string
        logo_path: string
        footer_note: string
    }

    type PlanItemKind = 'program' | 'spot' | 'break_filler' | 'day_filler'

    interface PlanItem {
        kind: PlanItemKind
        start_sec: number
        start_time: string
        source: string
        title: string
        duration: number
        break_window_id: number | null
        position_in_break: number | null
        spot_id: number | null
        campaign_id: number | null
    }

    interface PlanBreak {
        window_id: number
        nominal_time: string
        start_time: string
        distance_sec: number
        duration_sec: number
        spots_sec: number
        gap_sec: number
        spots: number
    }

    interface PlanWarning {
        code: string
        severity: 'warning' | 'error'
        message: string
        window_id: number | null
    }

    interface PlanPreview {
        date: string
        weekday: number
        playlist_exists: boolean
        plan: {
            items: PlanItem[]
            breaks: PlanBreak[]
            warnings: PlanWarning[]
            programs_sec: number
            breaks_sec: number
            total_sec: number
            day_length_sec: number
        }
    }

    interface Airing {
        id: number
        air_date: string
        kind: 'spot' | 'live'
        spot_id: number | null
        campaign_id: number | null
        advertiser_name: string
        campaign_name: string
        spot_title: string
        planned_start: string | null
        planned_duration_sec: number | null
        actual_start: string | null
        actual_end: string | null
        actual_duration_sec: number | null
        result: AirResult
    }

    interface ReportDayTotals {
        date: string
        planned: number
        aired: number
        partial: number
        missed: number
        pending: number
    }

    interface CommercialReport {
        header: {
            station_name: string
            advertiser: string | null
            campaign: string | null
            date_from: string
            date_to: string
            timezone: string
        }
        lines: unknown[]
        days: ReportDayTotals[]
        totals: ReportDayTotals
        notes: string[]
    }

    type AutoPlanState = 'created' | 'updated' | 'unchanged' | 'edited' | 'existing' | 'failed'

    interface AutoPlanSettings {
        enabled: boolean
        horizon_days: number
    }

    interface AutoPlanDay {
        plan_date: string
        state: AutoPlanState
        message: string
    }

    interface AutoPlanStatus {
        settings: AutoPlanSettings
        days: AutoPlanDay[]
    }

    interface AutoPlanRun {
        date: string
        state: AutoPlanState
        message: string
    }
}
