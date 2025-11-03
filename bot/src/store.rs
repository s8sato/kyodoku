use anyhow::Result;
use serenity::all::{ChannelId, GuildId, UserId};
use sqlx::{postgres::PgPoolOptions, FromRow, PgPool};

use crate::isbn::IsbnMetadata;

#[derive(Clone)]
pub struct Store {
    pool: PgPool,
}

impl Store {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await?;

        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok(Self { pool })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn upsert_isbn(&self, metadata: &IsbnMetadata) -> Result<()> {
        sqlx::query(
            "INSERT INTO isbn (isbn_13, isbn_10, title, subtitle, authors, source, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, NOW())
             ON CONFLICT (isbn_13) DO UPDATE SET
                 isbn_10 = EXCLUDED.isbn_10,
                 title = EXCLUDED.title,
                 subtitle = EXCLUDED.subtitle,
                 authors = EXCLUDED.authors,
                 source = EXCLUDED.source,
                 updated_at = NOW()",
        )
        .bind(&metadata.isbn_13)
        .bind(&metadata.isbn_10)
        .bind(&metadata.title)
        .bind(&metadata.subtitle)
        .bind(&metadata.authors)
        .bind(metadata.source.as_str())
        .execute(self.pool())
        .await?;

        Ok(())
    }

    pub async fn fetch_isbn(&self, isbn_13: &str) -> Result<Option<DbIsbn>> {
        let record = sqlx::query_as::<_, DbIsbn>(
            "SELECT isbn_13, isbn_10, title, subtitle, authors, source FROM isbn WHERE isbn_13 = $1",
        )
        .bind(isbn_13)
        .fetch_optional(self.pool())
        .await?;

        Ok(record)
    }

    pub async fn get_thread_id(
        &self,
        guild_id: GuildId,
        isbn_13: &str,
    ) -> Result<Option<ChannelId>> {
        let record: Option<(i64,)> = sqlx::query_as(
            "SELECT thread_id FROM isbn_threads WHERE guild_id = $1 AND isbn_13 = $2",
        )
        .bind(guild_id.get() as i64)
        .bind(isbn_13)
        .fetch_optional(self.pool())
        .await?;

        Ok(record.map(|(id,)| ChannelId::new(id as u64)))
    }

    pub async fn set_thread_id(
        &self,
        guild_id: GuildId,
        isbn_13: &str,
        thread_id: ChannelId,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO isbn_threads (guild_id, isbn_13, thread_id, updated_at)
             VALUES ($1, $2, $3, NOW())
             ON CONFLICT (guild_id, isbn_13) DO UPDATE SET
                 thread_id = EXCLUDED.thread_id,
                 updated_at = NOW()",
        )
        .bind(guild_id.get() as i64)
        .bind(isbn_13)
        .bind(thread_id.get() as i64)
        .execute(self.pool())
        .await?;

        Ok(())
    }

    pub async fn get_active_voice_channel(
        &self,
        guild_id: GuildId,
        isbn_13: &str,
    ) -> Result<Option<ChannelId>> {
        let record: Option<(i64,)> = sqlx::query_as(
            "SELECT channel_id FROM voice_sessions WHERE guild_id = $1 AND isbn_13 = $2 AND ended_at IS NULL",
        )
        .bind(guild_id.get() as i64)
        .bind(isbn_13)
        .fetch_optional(self.pool())
        .await?;

        Ok(record.map(|(id,)| ChannelId::new(id as u64)))
    }

    pub async fn start_voice_session(
        &self,
        guild_id: GuildId,
        channel_id: ChannelId,
        isbn_13: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO voice_sessions (guild_id, channel_id, isbn_13, started_at, ended_at)
             VALUES ($1, $2, $3, NOW(), NULL)
             ON CONFLICT (channel_id) DO UPDATE SET
                 guild_id = EXCLUDED.guild_id,
                 isbn_13 = EXCLUDED.isbn_13,
                 started_at = EXCLUDED.started_at,
                 ended_at = NULL",
        )
        .bind(guild_id.get() as i64)
        .bind(channel_id.get() as i64)
        .bind(isbn_13)
        .execute(self.pool())
        .await?;

        Ok(())
    }

    pub async fn end_voice_session(&self, channel_id: ChannelId) -> Result<()> {
        sqlx::query(
            "UPDATE voice_sessions SET ended_at = NOW() WHERE channel_id = $1 AND ended_at IS NULL",
        )
        .bind(channel_id.get() as i64)
        .execute(self.pool())
        .await?;

        Ok(())
    }

    pub async fn get_isbn_for_channel(&self, channel_id: ChannelId) -> Result<Option<String>> {
        let record: Option<(String,)> = sqlx::query_as(
            "SELECT isbn_13 FROM voice_sessions WHERE channel_id = $1 AND ended_at IS NULL",
        )
        .bind(channel_id.get() as i64)
        .fetch_optional(self.pool())
        .await?;

        Ok(record.map(|(isbn,)| isbn))
    }

    pub async fn add_watch(&self, guild_id: GuildId, user_id: UserId, isbn_13: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO watchlist (guild_id, user_id, isbn_13)
             VALUES ($1, $2, $3)
             ON CONFLICT DO NOTHING",
        )
        .bind(guild_id.get() as i64)
        .bind(user_id.get() as i64)
        .bind(isbn_13)
        .execute(self.pool())
        .await?;

        Ok(())
    }

    pub async fn remove_watch(
        &self,
        guild_id: GuildId,
        user_id: UserId,
        isbn_13: &str,
    ) -> Result<()> {
        sqlx::query("DELETE FROM watchlist WHERE guild_id = $1 AND user_id = $2 AND isbn_13 = $3")
            .bind(guild_id.get() as i64)
            .bind(user_id.get() as i64)
            .bind(isbn_13)
            .execute(self.pool())
            .await?;

        Ok(())
    }

    pub async fn list_watches(&self, guild_id: GuildId, user_id: UserId) -> Result<Vec<String>> {
        let records = sqlx::query_as::<_, (String,)>(
            "SELECT isbn_13 FROM watchlist WHERE guild_id = $1 AND user_id = $2 ORDER BY created_at",
        )
        .bind(guild_id.get() as i64)
        .bind(user_id.get() as i64)
        .fetch_all(self.pool())
        .await?;

        Ok(records.into_iter().map(|(isbn,)| isbn).collect())
    }

    pub async fn list_watchers(&self, guild_id: GuildId, isbn_13: &str) -> Result<Vec<UserId>> {
        let records = sqlx::query_as::<_, (i64,)>(
            "SELECT user_id FROM watchlist WHERE guild_id = $1 AND isbn_13 = $2",
        )
        .bind(guild_id.get() as i64)
        .bind(isbn_13)
        .fetch_all(self.pool())
        .await?;

        Ok(records
            .into_iter()
            .map(|(id,)| UserId::new(id as u64))
            .collect())
    }

    pub async fn get_notification_channel(&self, guild_id: GuildId) -> Result<Option<ChannelId>> {
        let record: Option<(i64,)> = sqlx::query_as(
            "SELECT notification_channel_id FROM guild_settings WHERE guild_id = $1",
        )
        .bind(guild_id.get() as i64)
        .fetch_optional(self.pool())
        .await?;

        Ok(record.and_then(|(id,)| {
            if id == 0 {
                None
            } else {
                Some(ChannelId::new(id as u64))
            }
        }))
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct DbIsbn {
    pub isbn_13: String,
    pub isbn_10: Option<String>,
    pub title: String,
    pub subtitle: Option<String>,
    pub authors: Vec<String>,
    pub source: String,
}
