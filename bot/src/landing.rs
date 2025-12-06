use anyhow::Result;
use serenity::all::{ChannelId, EditMessage, Message, UserId};
use serenity::builder::GetMessages;
use serenity::prelude::CacheHttp;
use tracing::{info, warn};

use crate::Config;

const EN_HEADER: &str = "# Welcome to the Shared Reading Server :books:";
const JA_HEADER: &str = "# ようこそ、共読サーバーへ :books:";

pub async fn refresh_landing_posts(
    ctx: &serenity::all::Context,
    config: &Config,
    bot_user_id: UserId,
) -> Result<()> {
    let Some(channel_id) = config.command_input_channel_id else {
        return Ok(());
    };

    let desired_posts = desired_posts(config);
    let mut landing_messages =
        fetch_existing_landing_messages(ctx, channel_id, bot_user_id).await?;

    let japanese = ensure_message(
        ctx,
        channel_id,
        landing_messages.remove(JA_HEADER),
        &desired_posts.0,
    )
    .await?;
    let english = ensure_message(
        ctx,
        channel_id,
        landing_messages.remove(EN_HEADER),
        &desired_posts.1,
    )
    .await?;

    if let Some(extra) = landing_messages.values().next() {
        warn!(
            "unexpected extra landing message left untouched: {}",
            extra.id
        );
    }

    info!(
        "refreshed landing posts (ja: {}, en: {})",
        japanese.id, english.id
    );

    Ok(())
}

fn desired_posts(config: &Config) -> (String, String) {
    let english = format!(
        concat!(
            "{header}\n",
            "A place where people reading the same book can loosely connect.\n\n",
            "## :book: Create a Reading Space\n",
            "### `/open <ISBN>`\n",
            "- Generates a **voice channel (ephemeral)** and a **text channel (persistent)** for the specified book\n",
            "- If the channels already exist, you will be guided to them\n",
            "- The voice channel automatically disappears after **{cleanup} seconds** of 0 participants\n\n",
            "## :eye_in_speech_bubble: Watchlist & Notifications\n",
            "### `/watch list`\n",
            "- Shows your current watchlist\n",
            "- When a watched book’s voice channel becomes active (the first participant joins), you will receive a **DM notification**\n",
            "### `/watch add <ISBN1> <ISBN2> ...`\n",
            "- Adds one or more books to your watchlist (up to **{watchlist_limit}** entries per user)\n",
            "### `/watch remove <ISBN1> <ISBN2> ...`\n",
            "- Removes one or more books from your watchlist\n\n",
            "## :bell: Enabling DM Notifications\n",
            "To receive active session alerts from the kyodoku app, please enable DMs in Discord:\n",
            "**User Settings → Content & Social → DIrect Messages → Allow DMs from other members in this server**\n",
            "Or:\n",
            "Right-click server name → **Privacy Settings → Allow DMs from other members in this server**\n\n",
            "## :file_cabinet: Channel Archival\n",
            "- Up to **{capacity}** text channels remain active; older ones move to the archive category\n",
            "- Archives are checked every **{archive_poll} seconds**\n",
            "- Archived channels are deleted after **{archive_grace} seconds** unless activity returns\n\n",
            "-# Take your time—and enjoy reading at your own pace.",
        ),
        header = EN_HEADER,
        cleanup = config.voice_cleanup_delay_seconds,
        capacity = config.text_channel_capacity,
        archive_poll = config.archive_poll_interval_seconds,
        archive_grace = config.archive_grace_period_seconds,
        watchlist_limit = config.watchlist_limit,
    );

    let japanese = format!(
        concat!(
            "{header}\n",
            "同じ本を読む人がゆるくつながるサーバーです。\n\n",
            "## :book: 読書空間の作成\n",
            "### `/open <ISBN>`\n",
            "- その本専用の **ボイスチャンネル** と **テキストチャンネル** を生成します\n",
            "- すでにチャンネルが存在する場合は、案内メッセージを表示します\n",
            "- ボイスチャンネルは参加者 0人の状態が **{cleanup} 秒** 続くと自動で消えます\n\n",
            "## :eye_in_speech_bubble: ウォッチ機能\n",
            "### `/watch list`\n",
            "- あなたのウォッチリストを表示します\n",
            "- ウォッチ中の本のボイスチャンネルがアクティブ（最初の参加者が入室）になると **DM 通知**が届きます\n",
            "### `/watch add <ISBN1> <ISBN2> ...`\n",
            "- ウォッチリストに本を追加します（複数指定可、1ユーザーあたり最大 **{watchlist_limit} 冊**）\n",
            "### `/watch remove <ISBN1> <ISBN2> ...`\n",
            "- ウォッチリストから本を削除します（複数指定可）\n\n",
            "## :bell: DM 通知を受け取るには\n",
            "kyodoku アプリからの通知を受信するために、Discord 側で DM を許可してください：\n",
            "**ユーザー設定 → コンテンツ＆ソーシャル → ダイレクトメッセージ → このサーバーのメンバーからのDMを許可**\n",
            "または：\n",
            "サーバー名を右クリック → **プライバシー設定 → このサーバーのメンバーからのDMを許可**\n\n",
            "## :file_cabinet: アーカイブに関する注意\n",
            "- **{capacity}** 件までテキストチャンネルを保持し、それ以上はアーカイブカテゴリへ移動します\n",
            "- **{archive_poll} 秒** ごとにアーカイブ対象を確認します\n",
            "- アーカイブ済みチャンネルは活動がなければ **{archive_grace} 秒** 後に削除されます\n\n",
            "-# どうぞ、あなたのペースで読書をお楽しみください。",
        ),
        header = JA_HEADER,
        cleanup = config.voice_cleanup_delay_seconds,
        capacity = config.text_channel_capacity,
        archive_poll = config.archive_poll_interval_seconds,
        archive_grace = config.archive_grace_period_seconds,
        watchlist_limit = config.watchlist_limit,
    );

    (japanese, english)
}

async fn fetch_existing_landing_messages(
    ctx: &serenity::all::Context,
    channel_id: ChannelId,
    bot_user_id: UserId,
) -> Result<std::collections::HashMap<&'static str, Message>> {
    let messages = channel_id
        .messages(ctx.http(), GetMessages::new().limit(50))
        .await?;

    let mut landing_messages = std::collections::HashMap::new();
    for message in messages
        .into_iter()
        .filter(|message| message.author.id == bot_user_id)
    {
        if message.content.starts_with(JA_HEADER) {
            landing_messages.entry(JA_HEADER).or_insert(message);
        } else if message.content.starts_with(EN_HEADER) {
            landing_messages.entry(EN_HEADER).or_insert(message);
        }
    }

    Ok(landing_messages)
}

async fn ensure_message(
    ctx: &serenity::all::Context,
    channel_id: ChannelId,
    existing: Option<Message>,
    content: &str,
) -> Result<Message> {
    let Some(mut message) = existing else {
        return Ok(channel_id.say(ctx.http(), content).await?);
    };

    if message.content != content {
        message
            .edit(ctx.http(), EditMessage::new().content(content))
            .await?;
    }

    Ok(message)
}
