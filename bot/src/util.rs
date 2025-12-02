use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use redis::AsyncCommands;
use serenity::all::{
    ButtonStyle, ChannelId, ChannelType, Context, CreateActionRow, CreateButton, CreateChannel,
    CreateMessage, EditChannel, GuildId, UserId,
};
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::isbn::IsbnMetadata;
use crate::BotState;

pub const WATCH_ACTION_PREFIX: &str = "watch:";
const CLEANUP_TTL_BUFFER_SECONDS: u64 = 60;
const ACTIVATION_TTL_SECONDS: i64 = 600;
const READING_SESSION_ACTIVATION_THRESHOLD: usize = 1;

pub async fn ensure_isbn_text_channel(
    ctx: &Context,
    guild_id: GuildId,
    metadata: &IsbnMetadata,
    state: Arc<BotState>,
) -> Result<ChannelId> {
    let channel_name = format!("{}（{}）", metadata.display_title(), metadata.isbn_13);
    let desired_name = truncate_name(&channel_name);
    let desired_topic = format!("Discussion channel for {}", metadata.display_title());

    if let Some(channel_id) = state
        .store
        .get_text_channel_id(guild_id, &metadata.isbn_13)
        .await?
    {
        if let Ok(channel) = ctx.http.get_channel(channel_id).await {
            if let Some(guild_channel) = channel.guild() {
                let mut edits = EditChannel::new();
                let mut needs_update = false;

                if guild_channel.name != desired_name {
                    edits = edits.name(desired_name.clone());
                    needs_update = true;
                }

                if guild_channel.topic.as_deref() != Some(desired_topic.as_str()) {
                    edits = edits.topic(desired_topic.clone());
                    needs_update = true;
                }

                if let Some(category_id) = state.config.text_channel_category_id {
                    if guild_channel.parent_id != Some(category_id) {
                        edits = edits.category(category_id);
                    }

                    edits = edits.position(0);
                    needs_update = true;
                }

                if needs_update {
                    guild_channel.id.edit(&ctx.http, edits).await?;
                }

                return Ok(guild_channel.id);
            }
        }
    }

    let mut channel = CreateChannel::new(desired_name)
        .kind(ChannelType::Text)
        .topic(desired_topic);
    if let Some(category_id) = state.config.text_channel_category_id {
        channel = channel.category(category_id);
    }
    let channel = guild_id.create_channel(&ctx.http, channel).await?;

    if let Some(category_id) = state.config.text_channel_category_id {
        move_channel_to_category_top(ctx, channel.id, category_id).await?;
    }

    let components = vec![CreateActionRow::Buttons(vec![CreateButton::new(format!(
        "{WATCH_ACTION_PREFIX}add:{}",
        metadata.isbn_13
    ))
    .style(ButtonStyle::Primary)
    .label("ウォッチリストに追加")])];

    channel
        .id
        .send_message(
            &ctx.http,
            CreateMessage::new()
                .content(format_metadata_post(metadata))
                .components(components),
        )
        .await?;

    state
        .store
        .set_text_channel_id(guild_id, &metadata.isbn_13, channel.id)
        .await?;

    Ok(channel.id)
}

pub async fn ensure_isbn_voice_channel(
    ctx: &Context,
    guild_id: GuildId,
    metadata: &IsbnMetadata,
    state: Arc<BotState>,
) -> Result<ChannelId> {
    let channel_name = format!("{}（{}）", metadata.display_title(), metadata.isbn_13);
    let desired_name = truncate_name(&channel_name);

    if let Some(channel_id) = state
        .store
        .get_active_voice_channel(guild_id, &metadata.isbn_13)
        .await?
    {
        if let Ok(channel) = ctx.http.get_channel(channel_id).await {
            if let Some(guild_channel) = channel.guild() {
                let mut edits = EditChannel::new();
                let mut needs_update = false;

                if guild_channel.name != desired_name {
                    edits = edits.name(desired_name.clone());
                    needs_update = true;
                }

                if let Some(category_id) = state.config.voice_channel_category_id {
                    if guild_channel.parent_id != Some(category_id) {
                        edits = edits.category(category_id);
                    }

                    edits = edits.position(0);
                    needs_update = true;
                }

                if needs_update {
                    guild_channel.id.edit(&ctx.http, edits).await?;
                }

                return Ok(guild_channel.id);
            }
        }
    }

    let mut voice = CreateChannel::new(desired_name).kind(ChannelType::Voice);
    if let Some(category_id) = state.config.voice_channel_category_id {
        voice = voice.category(category_id);
    }
    let voice = guild_id.create_channel(&ctx.http, voice).await?;

    if let Some(category_id) = state.config.voice_channel_category_id {
        move_channel_to_category_top(ctx, voice.id, category_id).await?;
    }

    state
        .store
        .start_voice_session(guild_id, voice.id, &metadata.isbn_13)
        .await?;
    schedule_cleanup(ctx.clone(), state.clone(), guild_id, voice.id).await?;

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
    let cleanup_delay = state.config.voice_cleanup_delay_seconds;
    let cleanup_ttl = cleanup_delay.saturating_add(CLEANUP_TTL_BUFFER_SECONDS);
    let mut conn = state.redis.get_async_connection().await?;
    let key = format!("voice:cleanup:{}", channel_id.get());
    let inserted: bool = conn.set_nx(&key, 1).await?;
    if inserted {
        let _: bool = conn.expire(&key, cleanup_ttl as i64).await?;
        let ctx_clone = ctx.clone();
        let state_clone = state.clone();
        let key_clone = key.clone();
        tokio::spawn(async move {
            sleep(Duration::from_secs(cleanup_delay)).await;
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

async fn move_channel_to_category_top(
    ctx: &Context,
    channel_id: ChannelId,
    category_id: ChannelId,
) -> Result<()> {
    channel_id
        .edit(
            &ctx.http,
            EditChannel::new().category(category_id).position(0),
        )
        .await?;

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

    if member_count < READING_SESSION_ACTIVATION_THRESHOLD {
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
    voice_ch_id: ChannelId,
) -> Result<()> {
    let Some(isbn) = state.store.get_isbn_for_voice_channel(voice_ch_id).await? else {
        return Ok(());
    };

    let watchers = state.store.list_watchers(guild_id, &isbn).await?;
    if watchers.is_empty() {
        return Ok(());
    }

    let record = state.store.fetch_isbn(&isbn).await?;
    let entry = record
        .as_ref()
        .map(|db| {
            let title = match &db.subtitle {
                Some(sub) if !sub.is_empty() => format!("{}: {}", db.title, sub),
                _ => db.title.clone(),
            };
            format!("**{}**(`{}`)", title, isbn)
        })
        .unwrap_or_else(|| format!("Session `{}`", isbn));

    let text_ch_id = state.store.get_text_channel_id(guild_id, &isbn).await?;
    let content = format_dm_content(&entry);

    for watcher in watchers {
        let dm_components = build_channel_buttons(
            guild_id,
            text_ch_id,
            voice_ch_id,
            Some(unwatch_custom_id(guild_id, &isbn)),
        );

        if let Err(err) = send_dm(&ctx.http, watcher, &content, dm_components).await {
            warn!(
                "failed to send DM for reading session {} to {}: {err:?}",
                isbn, watcher
            );
        }
    }

    Ok(())
}

fn format_dm_content(entry: &str) -> String {
    let mut content = format!("Reading session for {} is now active!\n", entry);

    content.push_str("Use the buttons below to join the session.");

    content
}

fn format_metadata_post(metadata: &IsbnMetadata) -> String {
    let mut content = format!("**{}**\n", metadata.display_title());

    if !metadata.authors.is_empty() {
        content.push_str(&format!("{}\n", metadata.authors.join(", ")));
    }

    content.push_str(&format!("ISBN-13: `{}`\n", metadata.isbn_13));

    let amazon_link = if let Some(isbn_10) = &metadata.isbn_10 {
        format!("https://www.amazon.co.jp/dp/{}", isbn_10)
    } else {
        format!("https://www.amazon.co.jp/s?k={}", metadata.isbn_13)
    };
    content.push_str(&format!("{amazon_link}"));

    content.push_str(
        "\n\nこの本のセッションがアクティブになったときに通知を受け取るには、下のボタンからウォッチリストに追加してください。",
    );

    content
}

async fn send_dm(
    http: &serenity::http::Http,
    user_id: UserId,
    content: &str,
    components: Vec<CreateActionRow>,
) -> Result<()> {
    let channel = user_id.create_dm_channel(http).await?;

    channel
        .id
        .send_message(
            http,
            CreateMessage::new().content(content).components(components),
        )
        .await?;

    Ok(())
}

pub fn build_channel_buttons(
    guild_id: GuildId,
    text_channel: Option<ChannelId>,
    voice_channel: ChannelId,
    remove_custom_id: Option<String>,
) -> Vec<CreateActionRow> {
    let mut buttons = vec![CreateButton::new_link(channel_url(guild_id, voice_channel))
        .label("ボイスチャンネルに参加")];

    if let Some(text_channel) = text_channel {
        buttons.push(
            CreateButton::new_link(channel_url(guild_id, text_channel))
                .label("テキストチャンネルを開く"),
        );
    }

    if let Some(custom_id) = remove_custom_id {
        buttons.push(
            CreateButton::new(custom_id)
                .style(ButtonStyle::Danger)
                .label("ウォッチリストから削除"),
        );
    }

    vec![CreateActionRow::Buttons(buttons)]
}

fn channel_url(guild_id: GuildId, channel_id: ChannelId) -> String {
    format!(
        "https://discord.com/channels/{}/{}",
        guild_id.get(),
        channel_id.get()
    )
}

fn unwatch_custom_id(guild_id: GuildId, isbn_13: &str) -> String {
    format!(
        "{}remove:{}:{}",
        WATCH_ACTION_PREFIX,
        guild_id.get(),
        isbn_13
    )
}

fn truncate_name(name: &str) -> String {
    const MAX_LEN: usize = 90;
    const SUFFIX: &str = "…";

    if name.len() <= MAX_LEN {
        return name.to_string();
    }

    let max_without_suffix = MAX_LEN.saturating_sub(SUFFIX.len());
    let mut truncated = String::new();

    for (idx, ch) in name.char_indices() {
        if idx + ch.len_utf8() > max_without_suffix {
            break;
        }
        truncated.push(ch);
    }

    truncated.push_str(SUFFIX);
    truncated
}

#[cfg(test)]
mod tests {
    use super::truncate_name;

    #[test]
    fn truncate_name_preserves_ascii_boundaries() {
        let name = "a".repeat(100);
        let truncated = truncate_name(&name);

        assert_eq!(truncated.len(), 90);
        assert!(truncated.starts_with(&"a".repeat(87)));
        assert_eq!(truncated.chars().last(), Some('…'));
    }

    #[test]
    fn truncate_name_preserves_char_boundaries_for_multibyte() {
        // "本" is 3 bytes; ensure we don't panic and keep complete characters.
        let name = "本".repeat(50); // 150 bytes
        let truncated = truncate_name(&name);

        assert!(truncated.len() < name.len());
        assert!(truncated.starts_with(&"本".repeat(29)));
        assert_eq!(truncated.chars().last(), Some('…'));
    }
}
