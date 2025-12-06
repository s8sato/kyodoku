use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use chrono::{DateTime, TimeDelta, Utc};
use serenity::all::{ChannelId, ChannelType, EditChannel, GuildChannel, GuildId};
use serenity::http::Http;
use tracing::{error, info, warn};

use crate::store::DbArchivedChannel;
use crate::BotState;

pub async fn run_archive_loop(http: Arc<Http>, state: Arc<BotState>) {
    let poll_interval = Duration::from_secs(state.config.archive_poll_interval_seconds);

    loop {
        if let Err(err) = archive_cycle(&http, state.clone()).await {
            error!("Archive cycle failed: {err:?}");
        }

        tokio::time::sleep(poll_interval).await;
    }
}

async fn archive_cycle(http: &Http, state: Arc<BotState>) -> Result<()> {
    let guilds = http.get_guilds(None, None).await?;

    for guild in guilds {
        if let Err(err) = process_guild(http, guild.id, state.clone()).await {
            error!("Archive processing failed for guild {}: {err:?}", guild.id);
        }
    }

    Ok(())
}

async fn process_guild(http: &Http, guild_id: GuildId, state: Arc<BotState>) -> Result<()> {
    let channels = guild_id.channels(http).await?;

    let mut text_channels: Vec<_> = channels
        .values()
        .filter(|ch| {
            ch.kind == ChannelType::Text
                && ch.parent_id == Some(state.config.text_channel_category_id)
        })
        .cloned()
        .collect();
    text_channels.sort_by_key(|ch| (ch.position, ch.id));

    let archive_grace = Duration::from_secs(state.config.archive_grace_period_seconds);

    for channel in text_channels
        .iter()
        .skip(state.config.text_channel_capacity)
    {
        if let Err(err) = archive_channel(http, channel, archive_grace, state.clone()).await {
            error!("Failed to archive channel {}: {err:?}", channel.id);
        }
    }

    for channel in channels.values().filter(|ch| {
        ch.kind == ChannelType::Text
            && ch.parent_id == Some(state.config.archived_channel_category_id)
    }) {
        handle_archived_channel(http, channel, archive_grace, state.clone()).await?;
    }

    Ok(())
}

async fn archive_channel(
    http: &Http,
    channel: &GuildChannel,
    archive_grace: Duration,
    state: Arc<BotState>,
) -> Result<()> {
    let archived_at: DateTime<Utc> = SystemTime::now().into();
    let grace_delta = archive_grace_delta(archive_grace);
    let expires_at = archived_at + grace_delta;
    let topic = format_archive_topic(archived_at.into(), archive_grace);

    state
        .store
        .upsert_archived_channel(
            channel.guild_id,
            channel.id,
            state.config.text_channel_category_id,
            archived_at,
            expires_at,
        )
        .await?;

    channel
        .id
        .edit(
            http,
            EditChannel::new()
                .category(state.config.archived_channel_category_id)
                .topic(topic),
        )
        .await?;

    Ok(())
}

async fn handle_archived_channel(
    http: &Http,
    channel: &GuildChannel,
    archive_grace: Duration,
    state: Arc<BotState>,
) -> Result<()> {
    let mut record = ensure_archived_record(channel, archive_grace, state.clone()).await?;
    let grace_delta = archive_grace_delta(archive_grace);
    let computed_expires_at = record.archived_at + grace_delta;

    if computed_expires_at != record.expires_at {
        record.expires_at = computed_expires_at;
        state
            .store
            .upsert_archived_channel(
                GuildId::new(record.guild_id as u64),
                ChannelId::new(record.channel_id as u64),
                ChannelId::new(record.original_category_id as u64),
                record.archived_at,
                record.expires_at,
            )
            .await?;
    }

    if computed_expires_at <= Utc::now() {
        info!("Deleting expired archived channel {}", channel.id);
        if let Err(err) = channel.id.delete(http).await {
            warn!("Unable to delete archived channel {}: {err:?}", channel.id);
        }
        if let Err(err) = state.store.clear_archived_channel(channel.id).await {
            warn!("Failed to clear archive record for {}: {err:?}", channel.id);
        }
        return Ok(());
    }

    let formatted_topic = format_archive_topic(record.archived_at.into(), archive_grace);
    if channel.topic.as_deref() != Some(formatted_topic.as_str()) {
        if let Err(err) = channel
            .id
            .edit(http, EditChannel::new().topic(formatted_topic))
            .await
        {
            warn!("Failed to update archive topic for {}: {err:?}", channel.id);
        }
    }

    Ok(())
}

async fn ensure_archived_record(
    channel: &GuildChannel,
    archive_grace: Duration,
    state: Arc<BotState>,
) -> Result<DbArchivedChannel> {
    if let Some(record) = state.store.get_archived_channel(channel.id).await? {
        return Ok(record);
    }

    let archived_at: DateTime<Utc> = SystemTime::now().into();
    let grace_delta = archive_grace_delta(archive_grace);
    let expires_at = archived_at + grace_delta;

    state
        .store
        .upsert_archived_channel(
            channel.guild_id,
            channel.id,
            state.config.text_channel_category_id,
            archived_at,
            expires_at,
        )
        .await?;

    state
        .store
        .get_archived_channel(channel.id)
        .await?
        .context("archived channel record missing after creation")
}

pub fn format_archive_topic(archived_at: SystemTime, archive_grace: Duration) -> String {
    let archived_dt: DateTime<Utc> = archived_at.into();
    let grace_delta = archive_grace_delta(archive_grace);
    let expires_at = archived_dt + grace_delta;

    format!(
        "Archived on {}. Scheduled for deletion on {} unless reopened.",
        archived_dt.format("%F %T %Z"),
        expires_at.format("%F %T %Z")
    )
}

fn archive_grace_delta(archive_grace: Duration) -> TimeDelta {
    let grace_secs = archive_grace.as_secs().min(i64::MAX as u64);
    TimeDelta::try_seconds(grace_secs as i64)
        .unwrap_or_else(|| TimeDelta::try_seconds(i64::MAX).unwrap())
}
