-- Scheduled on-screen graphics (CG): a text or an image placed at an exact
-- spot of the picture, shown by itself at the scheduled times.
--
-- Position and size are percentages of the picture, so a graphic keeps its
-- place when the channel changes the output resolution.

CREATE TABLE cg (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    channel_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('text', 'image')),
    text TEXT NOT NULL DEFAULT '',
    -- Image file, relative to the channel storage.
    image_path TEXT NOT NULL DEFAULT '',
    -- Top left corner, in percent of the picture width and height.
    x_pct REAL NOT NULL DEFAULT 5,
    y_pct REAL NOT NULL DEFAULT 5,
    -- Text: letter height in percent of the picture height.
    -- Image: width in percent of the picture width.
    size_pct REAL NOT NULL DEFAULT 5,
    font_family TEXT NOT NULL DEFAULT '',
    font_weight TEXT NOT NULL DEFAULT 'bold' CHECK (font_weight IN ('normal', 'semibold', 'bold')),
    text_color TEXT NOT NULL DEFAULT '#ffffff',
    box_enabled INTEGER NOT NULL DEFAULT 0 CHECK (box_enabled IN (0, 1)),
    box_color TEXT NOT NULL DEFAULT '#000000',
    box_opacity REAL NOT NULL DEFAULT 0.6,
    opacity REAL NOT NULL DEFAULT 1,
    fade_in_seconds REAL NOT NULL DEFAULT 0,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    FOREIGN KEY (channel_id) REFERENCES channels(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE INDEX idx_cg_channel ON cg (channel_id);

CREATE TABLE cg_schedule (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cg_id INTEGER NOT NULL,
    -- clock: at a time of the day. program: after a program starts.
    mode TEXT NOT NULL CHECK (mode IN ('clock', 'program')),
    -- Monday is bit 0, Sunday bit 6.
    weekdays INTEGER NOT NULL DEFAULT 127 CHECK (weekdays BETWEEN 1 AND 127),
    date_from TEXT,
    date_to TEXT,
    start_time TEXT NOT NULL DEFAULT '00:00:00',
    -- Program mode: part of the program title; empty matches any program.
    program_match TEXT NOT NULL DEFAULT '',
    offset_seconds INTEGER NOT NULL DEFAULT 0 CHECK (offset_seconds >= 0),
    duration_seconds INTEGER NOT NULL DEFAULT 10 CHECK (duration_seconds > 0),
    -- Clock mode: shows again every so many seconds, until `until_time`.
    repeat_every_seconds INTEGER CHECK (repeat_every_seconds IS NULL OR repeat_every_seconds > 0),
    until_time TEXT,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    FOREIGN KEY (cg_id) REFERENCES cg(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE INDEX idx_cg_schedule_cg ON cg_schedule (cg_id);
