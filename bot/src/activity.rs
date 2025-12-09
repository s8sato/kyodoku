use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serenity::all::{ChannelId, Context, GuildChannel, GuildId};
use tokio::time::interval;
use tracing::{info, warn};

use crate::store::DbTextChannel;
use crate::util;
use crate::BotState;

#[derive(Clone)]
struct ChannelActivity {
    guild_id: GuildId,
    channel_id: ChannelId,
    parent_id: Option<ChannelId>,
    score: f64,
}

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

    let mut activities = Vec::with_capacity(text_channels.len());

    for channel in text_channels {
        match evaluate_channel(ctx, state, &channel, &watcher_counts).await {
            Ok(Some(activity)) => activities.push(activity),
            Ok(None) => {}
            Err(err) => {
                warn!(
                    "failed to evaluate activity for text channel {}: {err:?}",
                    channel.channel_id
                );
            }
        }
    }

    apply_text_category_transitions(ctx, state, activities).await?;

    Ok(())
}

async fn evaluate_channel(
    ctx: &Context,
    state: &Arc<BotState>,
    channel: &DbTextChannel,
    watcher_counts: &HashMap<(GuildId, String), usize>,
) -> Result<Option<ChannelActivity>> {
    let channel_id = ChannelId::new(channel.channel_id as u64);
    let Some(guild_channel) = fetch_guild_channel(ctx, channel_id).await else {
        return Ok(None);
    };

    let voice_channel_id = state
        .store
        .get_active_voice_session(guild_channel.guild_id, &channel.isbn_13)
        .await?;
    let lookback_seconds = state.config.text_activity_eval_interval_seconds;
    let voice_participants = if let Some(voice_channel_id) = voice_channel_id {
        let member_ids =
            util::current_voice_member_ids(ctx, guild_channel.guild_id, voice_channel_id);

        for user_id in &member_ids {
            state
                .store
                .record_voice_participation(guild_channel.guild_id, &channel.isbn_13, *user_id)
                .await?;
        }

        state
            .store
            .count_recent_voice_participants(
                guild_channel.guild_id,
                &channel.isbn_13,
                lookback_seconds,
            )
            .await?
    } else {
        0
    };

    let watcher_key = (guild_channel.guild_id, channel.isbn_13.clone());
    let watch_count = watcher_counts.get(&watcher_key).copied().unwrap_or(0);

    let presence_factor = state.config.text_activity_presence_factor;
    let score = watch_count as f64 + (voice_participants as f64 * presence_factor);

    let desired_topic = format!(
        "Activity score: {:.2} (watchers: {}, unique voice participants (last {}s): {}, presence factor: {:.2})",
        score, watch_count, lookback_seconds, voice_participants, presence_factor
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

    Ok(Some(ChannelActivity {
        guild_id: guild_channel.guild_id,
        channel_id,
        parent_id: guild_channel.parent_id,
        score,
    }))
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

fn sort_by_score_desc(channels: &mut Vec<ChannelActivity>) {
    channels.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.channel_id.cmp(&b.channel_id))
    });
}

async fn apply_text_category_transitions(
    ctx: &Context,
    state: &Arc<BotState>,
    activities: Vec<ChannelActivity>,
) -> Result<()> {
    let categories = &state.config.text_category_ids;
    if categories.is_empty() {
        return Ok(());
    }

    let mut per_guild: HashMap<GuildId, Vec<Vec<ChannelActivity>>> = HashMap::new();

    for activity in activities {
        if let Some(idx) = categories
            .iter()
            .position(|category_id| Some(*category_id) == activity.parent_id)
        {
            per_guild
                .entry(activity.guild_id)
                .or_insert_with(|| vec![Vec::new(); categories.len()])[idx]
                .push(activity);
        }
    }

    for (guild_id, category_states) in per_guild.iter_mut() {
        for category in category_states.iter_mut() {
            sort_by_score_desc(category);
        }

        apply_swaps(
            category_states,
            state.config.text_category_capacity,
            state.config.text_category_swap_count,
        );

        let deletions = prune_lowest_category(
            category_states,
            state.config.text_category_capacity,
            state.config.text_category_prune_count,
        );

        enforce_category_order(ctx, *guild_id, categories, category_states).await?;
        delete_pruned_channels(ctx, state, deletions).await?;
    }

    Ok(())
}

fn apply_swaps(categories: &mut [Vec<ChannelActivity>], capacity: usize, swap_count: usize) {
    if swap_count == 0 {
        return;
    }

    let swap_start = capacity.saturating_sub(swap_count);

    for tier in 0..categories.len().saturating_sub(1) {
        let (higher_slice, lower_slice) = categories.split_at_mut(tier + 1);
        let higher = &mut higher_slice[tier];
        let lower = &mut lower_slice[0];

        let promotions: Vec<_> = lower.iter().take(swap_count).cloned().collect();
        let promotion_ids: HashSet<_> = promotions
            .iter()
            .map(|activity| activity.channel_id)
            .collect();

        let demotions: Vec<_> = higher
            .iter()
            .cloned()
            .enumerate()
            .filter(|(idx, _)| *idx >= swap_start)
            .map(|(_, activity)| activity)
            .collect();
        let demotion_ids: HashSet<_> = demotions
            .iter()
            .map(|activity| activity.channel_id)
            .collect();

        higher.retain(|activity| !demotion_ids.contains(&activity.channel_id));
        lower.retain(|activity| !promotion_ids.contains(&activity.channel_id));

        higher.extend(promotions);
        lower.extend(demotions);

        sort_by_score_desc(higher);
        sort_by_score_desc(lower);
    }
}

fn prune_lowest_category(
    categories: &mut [Vec<ChannelActivity>],
    capacity: usize,
    prune_count: usize,
) -> Vec<ChannelActivity> {
    if categories.is_empty() || prune_count == 0 {
        return Vec::new();
    }

    let threshold = capacity.saturating_sub(prune_count);
    let lowest_idx = categories.len() - 1;
    let lowest_category = &mut categories[lowest_idx];

    let mut retained = Vec::with_capacity(lowest_category.len());
    let mut pruned = Vec::new();

    for (idx, activity) in lowest_category.drain(..).enumerate() {
        if idx >= threshold {
            pruned.push(activity);
        } else {
            retained.push(activity);
        }
    }

    categories[lowest_idx] = retained;

    pruned
}

async fn enforce_category_order(
    ctx: &Context,
    guild_id: GuildId,
    categories: &[ChannelId],
    states: &[Vec<ChannelActivity>],
) -> Result<()> {
    for (tier_idx, category_id) in categories.iter().enumerate() {
        if let Some(channels) = states.get(tier_idx) {
            for activity in channels {
                if activity.parent_id != Some(*category_id) {
                    activity
                        .channel_id
                        .edit(
                            &ctx.http,
                            serenity::all::EditChannel::new().category(*category_id),
                        )
                        .await?;
                }
            }

            let positions: Vec<_> = channels
                .iter()
                .enumerate()
                .map(|(idx, activity)| (activity.channel_id, idx as u64))
                .collect();

            guild_id.reorder_channels(&ctx.http, positions).await?;
        }
    }

    Ok(())
}

async fn delete_pruned_channels(
    ctx: &Context,
    state: &Arc<BotState>,
    deletions: Vec<ChannelActivity>,
) -> Result<()> {
    for activity in deletions {
        info!(channel_id = %activity.channel_id, "Pruning inactive text channel");

        if let Err(err) = activity.channel_id.delete(&ctx.http).await {
            warn!(
                channel_id = %activity.channel_id,
                "Failed to delete pruned channel: {err:?}"
            );
        }

        if let Err(err) = state.store.delete_text_channel(activity.channel_id).await {
            warn!(
                channel_id = %activity.channel_id,
                "Failed to remove pruned channel from store: {err:?}"
            );
        }
    }

    Ok(())
}
