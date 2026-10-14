-- Per-channel settings for the conversion of uploaded spots, plus the source
-- file and conversion progress of each spot.

CREATE TABLE commercial_settings (
    channel_id INTEGER PRIMARY KEY,
    -- How the audio tracks of a source file become the stereo program:
    --   auto            one track: use it; several mono tracks: first two as L/R;
    --                   otherwise the first track
    --   first_stream    only the first audio track
    --   first_two_mono  the first two tracks as left and right
    --   mix_all         every track mixed into both channels
    audio_mapping TEXT NOT NULL DEFAULT 'auto'
        CHECK (audio_mapping IN ('auto', 'first_stream', 'first_two_mono', 'mix_all')),
    loudness_enabled INTEGER NOT NULL DEFAULT 1 CHECK (loudness_enabled IN (0, 1)),
    -- Integrated loudness target in LUFS (LKFS) and true-peak ceiling in dBTP.
    loudness_target_lufs REAL NOT NULL DEFAULT -24.0
        CHECK (loudness_target_lufs BETWEEN -70.0 AND -5.0),
    loudness_true_peak_dbtp REAL NOT NULL DEFAULT -2.0
        CHECK (loudness_true_peak_dbtp BETWEEN -20.0 AND 0.0),
    FOREIGN KEY (channel_id) REFERENCES channels(id) ON UPDATE CASCADE ON DELETE CASCADE
);

INSERT INTO commercial_settings (channel_id)
SELECT id FROM channels;

-- The uploaded file stays where the operator put it (source_path); file_path
-- becomes the converted file the engine plays once the conversion is done.
ALTER TABLE spot ADD COLUMN source_path TEXT NOT NULL DEFAULT '';
ALTER TABLE spot ADD COLUMN progress INTEGER NOT NULL DEFAULT 0
    CHECK (progress BETWEEN 0 AND 100);

CREATE UNIQUE INDEX spot_channel_source
ON spot(channel_id, source_path)
WHERE active = 1 AND source_path != '';
