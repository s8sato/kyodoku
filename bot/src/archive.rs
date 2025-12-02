use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use chrono::{DateTime, TimeDelta, Utc};
use serenity::all::{ChannelId, ChannelType, EditChannel, GuildId};
use serenity::http::Http;
use tracing::{error, info, warn};

use crate::BotState;

const ARCHIVE_MARKER_PREFIX: &str = "[archived_at:";

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
        if let Err(err) = archive_channel(http, channel.id, archive_grace, state.clone()).await {
            error!("Failed to archive channel {}: {err:?}", channel.id);
        }
    }

    for channel in channels.values().filter(|ch| {
        ch.kind == ChannelType::Text
            && ch.parent_id == Some(state.config.archived_channel_category_id)
    }) {
        handle_archived_channel(http, channel.id, channel.topic.as_deref(), archive_grace).await?;
    }

    Ok(())
}

async fn archive_channel(
    http: &Http,
    channel_id: ChannelId,
    archive_grace: Duration,
    state: Arc<BotState>,
) -> Result<()> {
    let archived_at = SystemTime::now();
    let topic = format_archive_topic(archived_at, archive_grace);

    channel_id
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
    channel_id: ChannelId,
    topic: Option<&str>,
    archive_grace: Duration,
) -> Result<()> {
    let archived_at = parse_archived_at(topic).unwrap_or_else(|| SystemTime::now());
    let expires_at = archived_at
        .checked_add(archive_grace)
        .unwrap_or_else(|| SystemTime::UNIX_EPOCH + Duration::from_secs(u64::MAX));

    let now = SystemTime::now();
    if expires_at <= now {
        info!("Deleting expired archived channel {}", channel_id);
        if let Err(err) = channel_id.delete(http).await {
            warn!("Unable to delete archived channel {}: {err:?}", channel_id);
        }
        return Ok(());
    }

    let formatted_topic = format_archive_topic(archived_at, archive_grace);
    if topic != Some(formatted_topic.as_str()) {
        if let Err(err) = channel_id
            .edit(http, EditChannel::new().topic(formatted_topic))
            .await
        {
            warn!("Failed to update archive topic for {}: {err:?}", channel_id);
        }
    }

    Ok(())
}

pub fn format_archive_topic(archived_at: SystemTime, archive_grace: Duration) -> String {
    let archived_at_secs = archived_at
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let archived_dt: DateTime<Utc> = archived_at.into();
    let grace_secs = archive_grace.as_secs().min(i64::MAX as u64);
    let grace_delta = TimeDelta::try_seconds(grace_secs as i64)
        .unwrap_or_else(|| TimeDelta::try_seconds(i64::MAX).unwrap());
    let expires_at = archived_dt + grace_delta;

    format!(
        "{ARCHIVE_MARKER_PREFIX}{archived_at_secs}] Archived on {}. Scheduled for deletion on {} unless reopened.",
        archived_dt.format("%F %T %Z"),
        expires_at.format("%F %T %Z")
    )
}

pub fn parse_archived_at(topic: Option<&str>) -> Option<SystemTime> {
    let topic = topic?;
    let marker_start = topic.find(ARCHIVE_MARKER_PREFIX)?;
    let rest = &topic[marker_start + ARCHIVE_MARKER_PREFIX.len()..];
    let end = rest.find(']')?;
    let timestamp = rest[..end].parse::<u64>().ok()?;

    UNIX_EPOCH.checked_add(Duration::from_secs(timestamp))
}
