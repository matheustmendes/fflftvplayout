-- Commercials: advertisers, campaigns, spots and break windows.
-- Rows are archived (active = 0) instead of deleted, so the airing history
-- added by later migrations never loses its references.

CREATE TABLE advertiser (
    id INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    contact_name TEXT NOT NULL DEFAULT '',
    email TEXT NOT NULL DEFAULT '',
    notes TEXT NOT NULL DEFAULT '',
    active INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    FOREIGN KEY (channel_id) REFERENCES channels(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE UNIQUE INDEX advertiser_channel_name
ON advertiser(channel_id, name COLLATE NOCASE)
WHERE active = 1;

-- A standalone spot is a campaign of kind 'standalone' with a single spot, so
-- every spot belongs to exactly one campaign. Dates are ISO (YYYY-MM-DD).
CREATE TABLE campaign (
    id INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL,
    advertiser_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'campaign' CHECK (kind IN ('campaign', 'standalone')),
    start_date TEXT,
    end_date TEXT,
    active INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    weight INTEGER NOT NULL DEFAULT 1 CHECK (weight > 0),
    max_per_day INTEGER CHECK (max_per_day IS NULL OR max_per_day > 0),
    position TEXT NOT NULL DEFAULT 'any' CHECK (position IN ('any', 'first', 'last')),
    CHECK (
        kind = 'standalone'
        OR (start_date IS NOT NULL AND end_date IS NOT NULL)
    ),
    CHECK (start_date IS NULL OR end_date IS NULL OR start_date <= end_date),
    FOREIGN KEY (channel_id) REFERENCES channels(id) ON UPDATE CASCADE ON DELETE CASCADE,
    FOREIGN KEY (advertiser_id) REFERENCES advertiser(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE INDEX campaign_channel_period
ON campaign(channel_id, active, start_date, end_date);

CREATE TABLE spot (
    id INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL,
    campaign_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    file_path TEXT NOT NULL,
    original_filename TEXT NOT NULL DEFAULT '',
    duration_sec REAL CHECK (duration_sec IS NULL OR duration_sec > 0),
    status TEXT NOT NULL DEFAULT 'processing' CHECK (status IN ('processing', 'ready', 'failed')),
    error TEXT NOT NULL DEFAULT '',
    active INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_by INTEGER,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (status != 'ready' OR duration_sec IS NOT NULL),
    FOREIGN KEY (channel_id) REFERENCES channels(id) ON UPDATE CASCADE ON DELETE CASCADE,
    FOREIGN KEY (campaign_id) REFERENCES campaign(id) ON UPDATE CASCADE ON DELETE CASCADE,
    FOREIGN KEY (created_by) REFERENCES auth_user(id) ON UPDATE CASCADE ON DELETE SET NULL
);

CREATE INDEX spot_campaign ON spot(campaign_id, active, status);

CREATE UNIQUE INDEX spot_channel_file
ON spot(channel_id, file_path)
WHERE active = 1;

-- A break window is a fixed clock time where a commercial break is planned.
-- weekday follows ISO 8601 (1 = Monday ... 7 = Sunday).
CREATE TABLE break_window (
    id INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL,
    weekday INTEGER NOT NULL CHECK (weekday BETWEEN 1 AND 7),
    nominal_time TEXT NOT NULL CHECK (nominal_time GLOB '[0-2][0-9]:[0-5][0-9]:[0-5][0-9]'),
    duration_sec INTEGER NOT NULL CHECK (duration_sec > 0),
    tolerance_sec INTEGER NOT NULL DEFAULT 60 CHECK (tolerance_sec >= 0),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    FOREIGN KEY (channel_id) REFERENCES channels(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE UNIQUE INDEX break_window_slot
ON break_window(channel_id, weekday, nominal_time);
