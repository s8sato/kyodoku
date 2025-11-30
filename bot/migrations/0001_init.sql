CREATE TABLE IF NOT EXISTS books (
    isbn_13 TEXT PRIMARY KEY,
    isbn_10 TEXT,
    title TEXT NOT NULL,
    subtitle TEXT,
    authors TEXT[] NOT NULL DEFAULT '{}',
    source TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_isbn_isbn10 ON books (isbn_10) WHERE isbn_10 IS NOT NULL;

CREATE TABLE IF NOT EXISTS guilds (
    guild_id BIGINT PRIMARY KEY,
    notification_channel_id BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS text_channels (
    guild_id BIGINT NOT NULL,
    isbn_13 TEXT NOT NULL REFERENCES books (isbn_13) ON DELETE CASCADE,
    channel_id BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (guild_id, isbn_13)
);

CREATE TABLE IF NOT EXISTS watchlist (
    guild_id BIGINT NOT NULL,
    user_id BIGINT NOT NULL,
    isbn_13 TEXT NOT NULL REFERENCES books (isbn_13) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (guild_id, user_id, isbn_13)
);

CREATE TABLE IF NOT EXISTS voice_channels (
    id BIGSERIAL PRIMARY KEY,
    guild_id BIGINT NOT NULL,
    channel_id BIGINT NOT NULL UNIQUE,
    isbn_13 TEXT NOT NULL REFERENCES books (isbn_13) ON DELETE CASCADE,
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ended_at TIMESTAMPTZ
);
