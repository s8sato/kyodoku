use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, TimeDelta, Utc};
use chrono_tz::Tz;
use serenity::all::{
    ButtonStyle, ChannelId, ChannelType, CreateActionRow, CreateButton, CreateMessage, EditChannel,
    GuildChannel, GuildId,
};
use serenity::http::Http;
use tracing::{error, info, warn};

use crate::store::DbArchivedChannel;
use crate::util::{self, EXTEND_ACTION_PREFIX};
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

pub async fn enforce_channel_budgets(
    http: &Http,
    guild_id: GuildId,
    state: &BotState,
    reservation: BudgetReservation,
) -> Result<()> {
    reconcile_guild(http, guild_id, Arc::new(state.clone())).await?;

    let channels = guild_id.channels(http).await?;
    let projected_total =
        channels.len() + reservation.new_categories + reservation.new_text_channels;
    if projected_total > state.config.max_server_channels {
        return Err(anyhow!(
            "Creating this channel would exceed the server channel cap ({}).",
            state.config.max_server_channels
        ));
    }

    let text_channels = channels
        .values()
        .filter(|ch| ch.kind == ChannelType::Text)
        .count()
        + reservation.new_text_channels;
    if text_channels > state.config.text_channel_budget {
        return Err(anyhow!(
            "Creating this channel would exceed the text channel budget ({}).",
            state.config.text_channel_budget
        ));
    }

    if reservation.new_categories > 0
        && channels.len() + reservation.new_categories > state.config.max_server_channels
    {
        return Err(anyhow!(
            "Creating an additional category would exceed the server channel cap ({}).",
            state.config.max_server_channels
        ));
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub struct BudgetReservation {
    pub new_text_channels: usize,
    pub new_categories: usize,
}

async fn archive_cycle(http: &Http, state: Arc<BotState>) -> Result<()> {
    let guilds = http.get_guilds(None, None).await?;

    for guild in guilds {
        if let Err(err) = reconcile_guild(http, guild.id, state.clone()).await {
            error!("Archive processing failed for guild {}: {err:?}", guild.id);
        }
    }

    Ok(())
}

async fn reconcile_guild(http: &Http, guild_id: GuildId, state: Arc<BotState>) -> Result<()> {
    process_guild(http, guild_id, state).await
}

async fn process_guild(http: &Http, guild_id: GuildId, state: Arc<BotState>) -> Result<()> {
    let categories = util::ensure_text_category_pool(http, guild_id, &state).await?;
    let category_ids: HashSet<_> = categories.into_iter().map(|cat| cat.id).collect();

    let channels = guild_id.channels(http).await?;
    let mut text_channels: Vec<_> = channels
        .values()
        .filter(|ch| {
            ch.kind == ChannelType::Text
                && ch.parent_id.map(|id| category_ids.contains(&id)) == Some(true)
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
    let topic = format_archive_topic(archived_at.into(), expires_at, state.config.time_zone);
    let original_category = channel
        .parent_id
        .unwrap_or(state.config.text_channel_category_id);

    state
        .store
        .upsert_archived_channel(
            channel.guild_id,
            channel.id,
            original_category,
            archived_at,
            expires_at,
            None,
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

    if computed_expires_at > record.expires_at {
        record.expires_at = computed_expires_at;
        state
            .store
            .update_archive_expiration(
                ChannelId::new(record.channel_id as u64),
                record.expires_at,
                record.notice_sent_at,
            )
            .await?;
    }

    let now = Utc::now();
    let notice_delta = seconds_to_timedelta(state.config.text_channel_delete_notice_seconds);
    let notice_cutoff = record.expires_at - notice_delta;
    if record.notice_sent_at.is_none() && notice_cutoff <= now {
        send_delete_warning(http, channel, &record, state.clone()).await?;
        record.notice_sent_at = Some(now);

        let min_expiration = now + notice_delta;
        if record.expires_at < min_expiration {
            record.expires_at = min_expiration;
        }

        state
            .store
            .update_archive_expiration(
                ChannelId::new(record.channel_id as u64),
                record.expires_at,
                record.notice_sent_at,
            )
            .await?;
    }

    if record.expires_at <= now {
        info!("Deleting expired archived channel {}", channel.id);
        if let Err(err) = channel.id.delete(http).await {
            warn!("Unable to delete archived channel {}: {err:?}", channel.id);
        }
        if let Err(err) = state.store.clear_archived_channel(channel.id).await {
            warn!("Failed to clear archive record for {}: {err:?}", channel.id);
        }
        return Ok(());
    }

    let formatted_topic = format_archive_topic(
        record.archived_at.into(),
        record.expires_at,
        state.config.time_zone,
    );
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

async fn send_delete_warning(
    http: &Http,
    channel: &GuildChannel,
    record: &DbArchivedChannel,
    state: Arc<BotState>,
) -> Result<()> {
    let expires_local = record.expires_at.with_timezone(&state.config.time_zone);
    let content = format!(
        "This channel is scheduled for deletion on {}. Use /open or Extend to keep it alive.",
        expires_local.format("%F %T %Z")
    );

    let extend_button = CreateButton::new(format!("{}{}", EXTEND_ACTION_PREFIX, channel.id.get()))
        .style(ButtonStyle::Primary)
        .label("Extend");

    if let Err(err) = channel
        .id
        .send_message(
            http,
            CreateMessage::new()
                .content(&content)
                .components(vec![CreateActionRow::Buttons(vec![extend_button.clone()])]),
        )
        .await
    {
        warn!("Failed to post delete warning to {}: {err:?}", channel.id);
    }

    let Some((guild_id, isbn)) = state.store.get_text_channel_info(channel.id).await? else {
        return Ok(());
    };

    let watchers = state.store.list_watchers(guild_id, &isbn).await?;
    if watchers.is_empty() {
        return Ok(());
    }

    let entry = match state.store.fetch_isbn(&isbn).await? {
        Some(record) => {
            let title = match record.subtitle {
                Some(sub) if !sub.is_empty() => format!("{}: {}", record.title, sub),
                _ => record.title,
            };
            format!("**{}** (`{}`)", title, record.isbn_13)
        }
        None => format!("`{isbn}`"),
    };

    let dm_content = format!(
        "{entry} is scheduled for deletion on {}. Tap Extend to keep it.",
        expires_local.format("%F %T %Z")
    );

    let mut buttons = vec![extend_button];
    buttons
        .push(CreateButton::new_link(channel_url(guild_id, channel.id)).label("Open Text Channel"));

    let components = vec![CreateActionRow::Buttons(buttons)];
    for watcher in watchers {
        if let Err(err) = util::send_dm(http, watcher, &dm_content, components.clone()).await {
            warn!(
                "Failed to DM deletion warning for channel {} to {}: {err:?}",
                channel.id, watcher
            );
        }
    }

    Ok(())
}

pub async fn extend_archived_channel(
    http: &Http,
    state: Arc<BotState>,
    channel_id: ChannelId,
) -> Result<String> {
    let Some(mut record) = state.store.get_archived_channel(channel_id).await? else {
        return Err(anyhow!("This channel is not scheduled for deletion."));
    };

    let extension = seconds_to_timedelta(state.config.text_channel_extension_seconds);
    let now = Utc::now();
    let new_expiration = (record.expires_at).max(now + extension);
    record.expires_at = new_expiration;
    record.notice_sent_at = None;

    state
        .store
        .update_archive_expiration(channel_id, record.expires_at, record.notice_sent_at)
        .await?;

    if let Ok(channel) = http.get_channel(channel_id).await {
        if let Some(guild_channel) = channel.guild() {
            let topic = format_archive_topic(
                record.archived_at.into(),
                record.expires_at,
                state.config.time_zone,
            );
            if let Err(err) = guild_channel
                .id
                .edit(http, EditChannel::new().topic(topic))
                .await
            {
                warn!(
                    "Failed to refresh topic after extension for {}: {err:?}",
                    channel_id
                );
            }
        }
    }

    Ok(format!(
        "Extended deletion deadline to {}.",
        record
            .expires_at
            .with_timezone(&state.config.time_zone)
            .format("%F %T %Z")
    ))
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
            channel
                .parent_id
                .unwrap_or(state.config.text_channel_category_id),
            archived_at,
            expires_at,
            None,
        )
        .await?;

    state
        .store
        .get_archived_channel(channel.id)
        .await?
        .context("archived channel record missing after creation")
}

pub fn format_archive_topic(
    archived_at: SystemTime,
    expires_at: DateTime<Utc>,
    time_zone: Tz,
) -> String {
    let archived_dt: DateTime<Utc> = archived_at.into();
    let archived_local = archived_dt.with_timezone(&time_zone);
    let expires_local = expires_at.with_timezone(&time_zone);

    format!(
        "Archived on {}. Scheduled for deletion on {} unless reopened or extended.",
        archived_local.format("%F %T %Z"),
        expires_local.format("%F %T %Z")
    )
}

fn archive_grace_delta(archive_grace: Duration) -> TimeDelta {
    let grace_secs = archive_grace.as_secs().min(i64::MAX as u64);
    TimeDelta::try_seconds(grace_secs as i64)
        .unwrap_or_else(|| TimeDelta::try_seconds(i64::MAX).unwrap())
}

fn seconds_to_timedelta(seconds: u64) -> TimeDelta {
    let bounded = seconds.min(i64::MAX as u64);
    TimeDelta::try_seconds(bounded as i64)
        .unwrap_or_else(|| TimeDelta::try_seconds(i64::MAX).unwrap())
}

fn channel_url(guild_id: GuildId, channel_id: ChannelId) -> String {
    format!(
        "https://discord.com/channels/{}/{}",
        guild_id.get(),
        channel_id.get()
    )
}
