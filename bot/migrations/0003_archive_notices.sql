ALTER TABLE archived_channels
ADD COLUMN IF NOT EXISTS notice_sent_at TIMESTAMPTZ;
