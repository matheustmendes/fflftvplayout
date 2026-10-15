-- Monitoring of the relay that serves the distribution destinations.
--
-- The relay (MediaMTX) has an API that lists who is connected. A destination is
-- one of the stations or sites that pull the signal; it is told apart by the
-- user it logs in with (RTMP) or by its address (SRT). The dashboard shows a
-- light for each one: connected or not.

CREATE TABLE relay_settings (
    channel_id INTEGER PRIMARY KEY,
    -- Base address of the relay API, e.g. http://127.0.0.1:9997. Empty: not monitored.
    api_url TEXT NOT NULL DEFAULT '',
    -- Path of the channel inside the relay, e.g. canal1.
    path TEXT NOT NULL DEFAULT '',
    FOREIGN KEY (channel_id) REFERENCES channels(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE TABLE monitor_destination (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    channel_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    protocol TEXT NOT NULL CHECK (protocol IN ('rtmp', 'srt')),
    match_user TEXT NOT NULL DEFAULT '',
    match_address TEXT NOT NULL DEFAULT '',
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    FOREIGN KEY (channel_id) REFERENCES channels(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE INDEX idx_monitor_destination_channel ON monitor_destination (channel_id);
