-- The weekly template: the ordered programs of each weekday. The planner
-- turns it, together with the break windows and the active campaigns, into
-- the playlist of a date.
-- weekday follows ISO 8601 (1 = Monday ... 7 = Sunday).

CREATE TABLE week_template_item (
    id INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL,
    weekday INTEGER NOT NULL CHECK (weekday BETWEEN 1 AND 7),
    position INTEGER NOT NULL CHECK (position >= 0),
    -- Path relative to the channel storage.
    source TEXT NOT NULL CHECK (source != ''),
    title TEXT NOT NULL DEFAULT '',
    in_sec REAL NOT NULL DEFAULT 0 CHECK (in_sec >= 0),
    out_sec REAL NOT NULL CHECK (out_sec > in_sec),
    -- Duration of the whole file at the time it was added.
    duration_sec REAL NOT NULL CHECK (duration_sec > 0),
    FOREIGN KEY (channel_id) REFERENCES channels(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE UNIQUE INDEX week_template_position
ON week_template_item(channel_id, weekday, position);
