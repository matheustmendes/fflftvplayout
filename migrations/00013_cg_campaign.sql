-- Graphics (CG) in the airing log and in campaigns.
--
-- A graphic can belong to a campaign (a QR code, a sponsor badge). Then it only
-- airs inside the dates of the campaign, and every time it appears it is
-- recorded in the airing log with kind 'cg', so the reports and the retention
-- rules of the campaign apply to it like to a spot.

ALTER TABLE cg ADD COLUMN campaign_id INTEGER
    REFERENCES campaign(id) ON UPDATE CASCADE ON DELETE SET NULL;

CREATE INDEX idx_cg_campaign ON cg (campaign_id);

-- SQLite cannot change a CHECK constraint, so the log table is rebuilt.
CREATE TABLE airlog_new (
    id INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL,
    -- Date of the playlist (YYYY-MM-DD), not of the calendar day of the airing.
    air_date TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'spot' CHECK (kind IN ('spot', 'live', 'cg')),
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

INSERT INTO airlog_new (id, channel_id, air_date, kind, spot_id, campaign_id, advertiser_id, advertiser_name, campaign_name, spot_title, file_path, break_window_id, position_in_break, planned_begin_sec, planned_start, planned_duration_sec, actual_start, actual_end, actual_duration_sec, result, reason, created_at)
SELECT id, channel_id, air_date, kind, spot_id, campaign_id, advertiser_id, advertiser_name, campaign_name, spot_title, file_path, break_window_id, position_in_break, planned_begin_sec, planned_start, planned_duration_sec, actual_start, actual_end, actual_duration_sec, result, reason, created_at FROM airlog;

DROP TABLE airlog;

ALTER TABLE airlog_new RENAME TO airlog;

CREATE INDEX airlog_date ON airlog(channel_id, air_date);

CREATE INDEX airlog_campaign ON airlog(channel_id, campaign_id, actual_start);

CREATE INDEX airlog_open ON airlog(channel_id, result, planned_start);
