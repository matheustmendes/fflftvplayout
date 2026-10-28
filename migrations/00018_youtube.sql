-- YouTube channels the playout may take videos from. Only the global
-- administrator edits the list, so a channel administrator can import from
-- the channels of the client but cannot add somebody else's.

CREATE TABLE youtube_source (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    channel_id INTEGER NOT NULL,
    -- The id YouTube gives the channel (UC...), not the @handle, which can change.
    youtube_channel_id TEXT NOT NULL,
    name TEXT NOT NULL DEFAULT '',
    FOREIGN KEY (channel_id) REFERENCES channels(id) ON UPDATE CASCADE ON DELETE CASCADE,
    UNIQUE (channel_id, youtube_channel_id)
);
