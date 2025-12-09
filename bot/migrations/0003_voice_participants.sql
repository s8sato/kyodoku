CREATE TABLE IF NOT EXISTS voice_participants (
    guild_id BIGINT NOT NULL,
    isbn_13 TEXT NOT NULL REFERENCES books (isbn_13) ON DELETE CASCADE,
    user_id BIGINT NOT NULL,
    last_seen TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (guild_id, isbn_13, user_id)
);
