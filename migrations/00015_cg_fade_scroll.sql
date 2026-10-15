-- Graphics: soft exit and sliding text.
ALTER TABLE cg ADD COLUMN fade_out_seconds REAL NOT NULL DEFAULT 0;

ALTER TABLE cg ADD COLUMN scroll TEXT NOT NULL DEFAULT 'none'
    CHECK (scroll IN ('none', 'right_to_left', 'left_to_right'));

-- Speed of a sliding text, in percent of the picture width per second.
ALTER TABLE cg ADD COLUMN scroll_speed_pct REAL NOT NULL DEFAULT 10;
