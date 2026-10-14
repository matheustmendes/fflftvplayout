-- Automatic planning of the upcoming days from the weekly schedule.
--
-- A background task keeps the next days of each channel planned. It writes a
-- day that has no playlist yet and keeps a day it wrote itself up to date
-- (a spot uploaded in the afternoon still enters tomorrow), but it never
-- touches a playlist an operator edited, nor the day on the air.

CREATE TABLE auto_plan_settings (
    channel_id INTEGER PRIMARY KEY,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    -- How many days after the day on the air are kept planned.
    horizon_days INTEGER NOT NULL DEFAULT 3 CHECK (horizon_days BETWEEN 1 AND 14),
    FOREIGN KEY (channel_id) REFERENCES channels(id) ON UPDATE CASCADE ON DELETE CASCADE
);

INSERT INTO auto_plan_settings (channel_id)
SELECT id FROM channels;

-- What the task did with each day. The fingerprint identifies the playlist the
-- task wrote: a playlist with another fingerprint was changed by somebody else.
CREATE TABLE auto_plan_day (
    channel_id INTEGER NOT NULL,
    plan_date TEXT NOT NULL,
    state TEXT NOT NULL CHECK (
        state IN ('created', 'updated', 'unchanged', 'edited', 'existing', 'failed')
    ),
    message TEXT NOT NULL DEFAULT '',
    fingerprint TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (channel_id, plan_date),
    FOREIGN KEY (channel_id) REFERENCES channels(id) ON UPDATE CASCADE ON DELETE CASCADE
);
