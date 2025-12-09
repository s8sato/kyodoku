use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serenity::all::{ChannelId, Context, GuildChannel};
use tokio::time::interval;
use tracing::{info, warn};

use crate::store::DbTextChannel;
use crate::util;
use crate::BotState;

pub async fn run_text_activity_eval_loop(ctx: Context, state: Arc<BotState>) -> Result<()> {
    let mut ticker = interval(Duration::from_secs(
        state.config.text_activity_eval_interval_seconds,
    ));

    loop {
        ticker.tick().await;

        if let Err(err) = evaluate_once(&ctx, &state).await {
            warn!("text activity evaluation failed: {err:?}");
        }
    }
}

async fn evaluate_once(ctx: &Context, state: &Arc<BotState>) -> Result<()> {
    let text_channels = state.store.list_text_channels().await?;
    let watcher_counts = state.store.list_watcher_counts().await?;

    for channel in text_channels {
        if let Err(err) = evaluate_channel(ctx, state, &channel, &watcher_counts).await {
            warn!(
                "failed to evaluate activity for text channel {}: {err:?}",
                channel.channel_id
            );
        }
    }

    Ok(())
}

async fn evaluate_channel(
    ctx: &Context,
    state: &Arc<BotState>,
    channel: &DbTextChannel,
    watcher_counts: &HashMap<(serenity::all::GuildId, String), usize>,
) -> Result<()> {
    let channel_id = ChannelId::new(channel.channel_id as u64);
    let Some(guild_channel) = fetch_guild_channel(ctx, channel_id).await else {
        return Ok(());
    };

    let voice_channel_id = state
        .store
        .get_active_voice_session(guild_channel.guild_id, &channel.isbn_13)
        .await?;
    let voice_participants = voice_channel_id
        .map(|voice_ch| util::current_voice_members(ctx, guild_channel.guild_id, voice_ch))
        .unwrap_or(0);

    let watcher_key = (guild_channel.guild_id, channel.isbn_13.clone());
    let watch_count = watcher_counts.get(&watcher_key).copied().unwrap_or(0);

    let presence_factor = state.config.text_activity_presence_factor;
    let score = watch_count as f64 + (voice_participants as f64 * presence_factor);

    let desired_topic = format!(
        "Activity score: {:.2} (watchers: {}, voice participants: {}, presence factor: {:.2})",
        score, watch_count, voice_participants, presence_factor
    );

    if guild_channel.topic.as_deref() != Some(desired_topic.as_str()) {
        guild_channel
            .id
            .edit(
                &ctx.http,
                serenity::all::EditChannel::new().topic(desired_topic),
            )
            .await?;
    }

    info!(
        guild_id = %guild_channel.guild_id,
        channel_id = %channel.channel_id,
        score, watch_count, voice_participants,
        "updated text channel activity"
    );

    Ok(())
}

async fn fetch_guild_channel(ctx: &Context, channel_id: ChannelId) -> Option<GuildChannel> {
    match ctx.http.get_channel(channel_id).await {
        Ok(channel) => channel.guild(),
        Err(err) => {
            warn!("failed to fetch channel {}: {err:?}", channel_id);
            None
        }
    }
}
