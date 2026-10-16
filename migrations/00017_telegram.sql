-- Telegram bot: alerts and read-only commands for a group of the team.
--
-- The bot is one for the whole installation (one token); each channel says which
-- group it talks to and which alerts it wants.

CREATE TABLE telegram_bot (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    token TEXT NOT NULL DEFAULT ''
);

INSERT INTO telegram_bot (id, token) VALUES (1, '');

CREATE TABLE telegram_settings (
    channel_id INTEGER PRIMARY KEY,
    enabled INTEGER NOT NULL DEFAULT 0,
    -- Id of the group (a negative number) or of a person.
    chat_id TEXT NOT NULL DEFAULT '',
    alert_output INTEGER NOT NULL DEFAULT 1,
    alert_destinations INTEGER NOT NULL DEFAULT 1,
    alert_filler INTEGER NOT NULL DEFAULT 1,
    alert_playlist INTEGER NOT NULL DEFAULT 1,
    alert_commercials INTEGER NOT NULL DEFAULT 1,
    alert_disk INTEGER NOT NULL DEFAULT 1,
    daily_summary INTEGER NOT NULL DEFAULT 1,
    -- Clock time of the channel (HH:MM).
    summary_time TEXT NOT NULL DEFAULT '08:00',
    -- The group may ask the bot for the state of the channel.
    commands_enabled INTEGER NOT NULL DEFAULT 1,
    FOREIGN KEY (channel_id) REFERENCES channels(id) ON UPDATE CASCADE ON DELETE CASCADE
);
