-- How the reports sent to advertisers present the station. Nothing here is
-- fixed in the code: every channel sets its own name, details, logo and note.

CREATE TABLE report_settings (
    channel_id INTEGER PRIMARY KEY,
    -- Empty means the name of the channel.
    station_name TEXT NOT NULL DEFAULT '',
    -- Address, tax id, contact: free text, one item per line.
    station_details TEXT NOT NULL DEFAULT '',
    -- PNG or JPEG inside the channel storage; empty means no logo.
    logo_path TEXT NOT NULL DEFAULT '',
    footer_note TEXT NOT NULL DEFAULT '',
    FOREIGN KEY (channel_id) REFERENCES channels(id) ON UPDATE CASCADE ON DELETE CASCADE
);

INSERT INTO report_settings (channel_id)
SELECT id FROM channels;
