-- A program of the weekly template can start at a fixed time ("enters at
-- 20:00"). NULL: it follows whatever comes before it, as before.
ALTER TABLE week_template_item ADD COLUMN fixed_start TEXT;
