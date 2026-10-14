-- The airing log: what was planned and what really went on the air.
--
-- A row is created when a plan is applied (result 'pending'), becomes
-- 'airing' when the spot starts, and ends in one of the final results. A spot
-- that airs without a plan (an operator added it by hand) gets a row without
-- planned fields. Live takeovers are logged as kind 'live' windows, so a spot
-- that never played can be explained by the live broadcast that took its time.
--
-- The names of the advertiser, campaign and spot are copied into the row, so
-- a report never changes when somebody renames them later.

CREATE TABLE airlog (
    id INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL,
    -- Date of the playlist (YYYY-MM-DD), not of the calendar day of the airing.
    air_date TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'spot' CHECK (kind IN ('spot', 'live')),
    spot_id INTEGER,
    campaign_id INTEGER,
    advertiser_id INTEGER,
    advertiser_name TEXT NOT NULL DEFAULT '',
    campaign_name TEXT NOT NULL DEFAULT '',
    spot_title TEXT NOT NULL DEFAULT '',
    file_path TEXT NOT NULL DEFAULT '',
    break_window_id INTEGER,
    position_in_break INTEGER,
    -- Seconds since midnight of air_date, as the playout counts them (it can
    -- pass 86400 for a day that starts in the evening).
    planned_begin_sec REAL,
    -- The same moment as a UTC timestamp (YYYY-MM-DDTHH:MM:SS.mmmZ).
    planned_start TEXT,
    planned_duration_sec REAL,
    actual_start TEXT,
    actual_end TEXT,
    actual_duration_sec REAL,
    result TEXT NOT NULL DEFAULT 'pending' CHECK (
        result IN (
            'pending', 'airing', 'played', 'partial', 'skipped', 'interrupted',
            'fallback', 'failed', 'not_aired', 'live_missed'
        )
    ),
    reason TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (channel_id) REFERENCES channels(id) ON UPDATE CASCADE ON DELETE CASCADE,
    FOREIGN KEY (spot_id) REFERENCES spot(id) ON UPDATE CASCADE ON DELETE SET NULL,
    FOREIGN KEY (campaign_id) REFERENCES campaign(id) ON UPDATE CASCADE ON DELETE SET NULL,
    FOREIGN KEY (advertiser_id) REFERENCES advertiser(id) ON UPDATE CASCADE ON DELETE SET NULL,
    FOREIGN KEY (break_window_id) REFERENCES break_window(id) ON UPDATE CASCADE ON DELETE SET NULL
);

CREATE INDEX airlog_date ON airlog(channel_id, air_date);

CREATE INDEX airlog_campaign ON airlog(channel_id, campaign_id, actual_start);

CREATE INDEX airlog_open ON airlog(channel_id, result, planned_start);
