use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use redis::AsyncCommands;
use serenity::all::{ChannelId, ChannelType, Context, CreateChannel, CreateMessage, GuildId};
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::isbn::IsbnMetadata;
use crate::BotState;

const CLEANUP_DELAY_SECONDS: u64 = 120;
const CLEANUP_TTL_SECONDS: i64 = 180;
const ACTIVATION_TTL_SECONDS: i64 = 600;

pub async fn ensure_isbn_thread(
    ctx: &Context,
    guild_id: GuildId,
    metadata: &IsbnMetadata,
    store: &crate::store::Store,
) -> Result<ChannelId> {
    if let Some(thread) = store.get_thread_id(guild_id, &metadata.isbn_13).await? {
        return Ok(thread);
    }

    let channel_name = format!("isbn-{}", metadata.isbn_13);
    let topic = format!("Discussion thread for {}", metadata.display_title());
    let channel = guild_id
        .create_channel(
            &ctx.http,
            CreateChannel::new(truncate_name(&channel_name))
                .kind(ChannelType::Text)
                .topic(topic),
        )
        .await?;

    store
        .set_thread_id(guild_id, &metadata.isbn_13, channel.id)
        .await?;

    Ok(channel.id)
}

pub async fn ensure_isbn_voice_channel(
    ctx: &Context,
    guild_id: GuildId,
    metadata: &IsbnMetadata,
    store: &crate::store::Store,
) -> Result<ChannelId> {
    if let Some(channel) = store
        .get_active_voice_channel(guild_id, &metadata.isbn_13)
        .await?
    {
        if ctx.http.get_channel(channel).await.is_ok() {
            return Ok(channel);
        }
    }

    let channel_name = format!("reading-{}", metadata.isbn_13);
    let voice = guild_id
        .create_channel(
            &ctx.http,
            CreateChannel::new(truncate_name(&channel_name)).kind(ChannelType::Voice),
        )
        .await?;

    store
        .start_voice_session(guild_id, voice.id, &metadata.isbn_13)
        .await?;

    Ok(voice.id)
}

pub async fn handle_voice_state_transition(
    ctx: &Context,
    state: Arc<BotState>,
    guild_id: GuildId,
    old: Option<ChannelId>,
    new: Option<ChannelId>,
) -> Result<()> {
    if let Some(channel_id) = old {
        schedule_cleanup(ctx.clone(), state.clone(), guild_id, channel_id).await?;
    }

    if let Some(channel_id) = new {
        maybe_notify_activation(ctx.clone(), state.clone(), guild_id, channel_id).await?;
    }

    Ok(())
}

async fn schedule_cleanup(
    ctx: Context,
    state: Arc<BotState>,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Result<()> {
    let mut conn = state.redis.get_async_connection().await?;
    let key = format!("voice:cleanup:{}", channel_id.get());
    let inserted: bool = conn.set_nx(&key, 1).await?;
    if inserted {
        let _: bool = conn.expire(&key, CLEANUP_TTL_SECONDS).await?;
        let ctx_clone = ctx.clone();
        let state_clone = state.clone();
        let key_clone = key.clone();
        tokio::spawn(async move {
            sleep(Duration::from_secs(CLEANUP_DELAY_SECONDS)).await;
            if let Err(err) =
                finalize_cleanup(ctx_clone, state_clone, guild_id, channel_id, key_clone).await
            {
                error!("failed to cleanup voice channel {}: {err:?}", channel_id);
            }
        });
    }
    Ok(())
}

async fn finalize_cleanup(
    ctx: Context,
    state: Arc<BotState>,
    guild_id: GuildId,
    channel_id: ChannelId,
    key: String,
) -> Result<()> {
    if current_voice_members(&ctx, guild_id, channel_id) > 0 {
        let mut conn = state.redis.get_async_connection().await?;
        let _: redis::RedisResult<i32> = conn.del(&key).await;
        return Ok(());
    }

    info!("Deleting inactive voice channel {}", channel_id);
    if let Err(err) = channel_id.delete(&ctx.http).await {
        warn!("Unable to delete channel {}: {err:?}", channel_id);
    }
    state.store.end_voice_session(channel_id).await?;

    let mut conn = state.redis.get_async_connection().await?;
    let _: redis::RedisResult<i32> = conn.del(&key).await;
    Ok(())
}

fn current_voice_members(ctx: &Context, guild_id: GuildId, channel_id: ChannelId) -> usize {
    if let Some(guild) = ctx.cache.guild(guild_id) {
        guild
            .voice_states
            .values()
            .filter(|state| state.channel_id == Some(channel_id))
            .count()
    } else {
        0
    }
}

async fn maybe_notify_activation(
    ctx: Context,
    state: Arc<BotState>,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Result<()> {
    let member_count = current_voice_members(&ctx, guild_id, channel_id);
    let threshold = state.config.reading_session_activation_threshold;

    if member_count < threshold {
        return Ok(());
    }

    let mut conn = state.redis.get_async_connection().await?;
    let key = format!("voice:active:{}", channel_id.get());
    let inserted: bool = conn.set_nx(&key, 1).await?;
    if !inserted {
        return Ok(());
    }
    let _: bool = conn.expire(&key, ACTIVATION_TTL_SECONDS).await?;

    let notify_result = notify_watchers(&ctx, &state, guild_id, channel_id).await;
    if let Err(err) = notify_result {
        error!(
            "failed to notify watchers for channel {}: {err:?}",
            channel_id
        );
        if let Err(del_err) = conn.del::<_, i32>(&key).await {
            warn!(
                "failed to reset activation flag for {}: {del_err:?}",
                channel_id
            );
        }
    }
    Ok(())
}

async fn notify_watchers(
    ctx: &Context,
    state: &BotState,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Result<()> {
    let Some(isbn) = state.store.get_isbn_for_channel(channel_id).await? else {
        return Ok(());
    };

    let watchers = state.store.list_watchers(guild_id, &isbn).await?;
    if watchers.is_empty() {
        return Ok(());
    }

    let record = state.store.fetch_isbn(&isbn).await?;
    let title = record
        .as_ref()
        .map(|db| match &db.subtitle {
            Some(sub) if !sub.is_empty() => format!("{}: {}", db.title, sub),
            _ => db.title.clone(),
        })
        .unwrap_or_else(|| format!("Session {}", isbn));

    let mut target_channel = state.store.get_notification_channel(guild_id).await?;
    if target_channel.is_none() {
        target_channel = state.store.get_thread_id(guild_id, &isbn).await?;
    }

    let Some(channel_id_target) = target_channel else {
        return Ok(());
    };

    let mentions = watchers
        .iter()
        .map(|id| format!("<@{}>", id))
        .collect::<Vec<_>>()
        .join(" ");

    let content = format!(
        "Reading session for **{}** is now active in <#{}>! {}",
        title, channel_id, mentions
    );

    channel_id_target
        .send_message(&ctx.http, CreateMessage::new().content(content))
        .await?;

    Ok(())
}

fn truncate_name(name: &str) -> String {
    const MAX_LEN: usize = 90;
    if name.len() > MAX_LEN {
        name[..MAX_LEN].to_string()
    } else {
        name.to_string()
    }
}
