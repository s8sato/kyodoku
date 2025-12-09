use anyhow::Result;
use serenity::all::{ChannelId, EditMessage, Message};
use serenity::builder::GetMessages;
use serenity::prelude::CacheHttp;
use tracing::info;

use crate::Config;

const EN_HEADER: &str = "# Welcome to the Shared Reading Server :books:";
const JA_HEADER: &str = "# ようこそ、共読サーバーへ :books:";
const MAX_MESSAGE_LENGTH: usize = 1900;

pub async fn refresh_landing_posts(ctx: &serenity::all::Context, config: &Config) -> Result<()> {
    let Some(channel_id) = config.command_input_channel_id else {
        return Ok(());
    };

    clear_command_channel(ctx, channel_id).await?;

    let desired_posts = desired_posts(config);

    let japanese = ensure_messages(ctx, channel_id, Vec::new(), &desired_posts.0).await?;
    let english = ensure_messages(ctx, channel_id, Vec::new(), &desired_posts.1).await?;

    info!(
        "refreshed landing posts (ja: {:?}, en: {:?})",
        japanese
            .iter()
            .map(|message| message.id.to_string())
            .collect::<Vec<_>>(),
        english
            .iter()
            .map(|message| message.id.to_string())
            .collect::<Vec<_>>()
    );

    Ok(())
}

fn desired_posts(config: &Config) -> (Vec<String>, Vec<String>) {
    let english_body = format!(
        concat!(
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
            "**User Settings → Content & Social → Direct Messages → Allow DMs from other members in this server**\n",
            "Or:\n",
            "Right-click server name → **Privacy Settings → Allow DMs from other members in this server**\n\n",
            "## :file_cabinet: Channel Archival\n",
            "- Up to **{capacity}** text channels remain active; older ones move to the archive category\n",
            "- Archives are checked every **{archive_poll} seconds**\n",
            "- Archived channels are deleted after **{archive_grace} seconds** unless activity returns\n\n",
            "## :zipper_mouth: Spoiler-Friendly Posts\n",
            "When you want to share impressions without revealing plot points, mark spoilers with `||double bars||`:\n",
            "```\n",
            "By the final page, I was shocked that ||the seemingly harmless character was the culprit all along||.\n\n",
            "After finishing, I'm still thinking about how the author ||leaves\n",
            "room\n",
            "for readers to interpret the ending through sparse descriptions and the placement of symbolic items in the last scene||. Overall, an amazing read!\n",
            "```\n",
            "> By the final page, I was shocked that ||the seemingly harmless character was the culprit all along||.\n",
            "> \n",
            "> After finishing, I'm still thinking about how the author ||leaves\n",
            "> room\n",
            "> for readers to interpret the ending through sparse descriptions and the placement of symbolic items in the last scene||. Overall, an amazing read!\n\n",
            "-# Take your time—and enjoy reading at your own pace.",
        ),
        cleanup = config.voice_cleanup_delay_seconds,
        capacity = config.active_text_channel_capacity,
        archive_poll = config.archive_poll_interval_seconds,
        archive_grace = config.archive_retention_seconds,
        watchlist_limit = config.watchlist_limit,
    );

    let japanese_body = format!(
        concat!(
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
            "## :zipper_mouth: ネタバレへの配慮\n",
            "文芸書などでネタバレを控えながら投稿したい場合、ネタバレ部分をスポイラーとしてマークできます：\n",
            "```\n",
            "最終ページをめくった感想ですが、||最後に明かされる真犯人が、これまで最も無害に見えた人物だった||という展開に驚かされました。\n\n",
            "読了後に考察すべき点として、作者が意図的に||結末の解釈を読者に委ねている\n",
            "余白\n",
            "の多い描写や、ラストシーンでの象徴的なアイテムの配置||があると感じました。全体的に素晴らしい読書体験でした！\n",
            "```\n",
            "> 最終ページをめくった感想ですが、||最後に明かされる真犯人が、これまで最も無害に見えた人物だった||という展開に驚かされました。\n",
            "> \n",
            "> 読了後に考察すべき点として、作者が意図的に||結末の解釈を読者に委ねている\n",
            "> 余白\n",
            "> の多い描写や、ラストシーンでの象徴的なアイテムの配置||があると感じました。全体的に素晴らしい読書体験でした！\n\n",
            "-# どうぞ、あなたのペースで読書をお楽しみください。",
        ),
        cleanup = config.voice_cleanup_delay_seconds,
        capacity = config.active_text_channel_capacity,
        archive_poll = config.archive_poll_interval_seconds,
        archive_grace = config.archive_retention_seconds,
        watchlist_limit = config.watchlist_limit,
    );

    (
        split_with_header(JA_HEADER, japanese_body),
        split_with_header(EN_HEADER, english_body),
    )
}

async fn clear_command_channel(ctx: &serenity::all::Context, channel_id: ChannelId) -> Result<()> {
    loop {
        let messages = channel_id
            .messages(ctx.http(), GetMessages::new().limit(100))
            .await?;

        if messages.is_empty() {
            break;
        }

        for message in messages {
            message.delete(ctx.http()).await?;
        }
    }

    info!("cleared command channel before posting landing messages");

    Ok(())
}

async fn ensure_messages(
    ctx: &serenity::all::Context,
    channel_id: ChannelId,
    existing: Vec<Message>,
    contents: &[String],
) -> Result<Vec<Message>> {
    let mut existing_iter = existing.into_iter();
    let mut synced = Vec::with_capacity(contents.len());

    for content in contents {
        if let Some(mut message) = existing_iter.next() {
            if message.content != *content {
                message
                    .edit(ctx.http(), EditMessage::new().content(content))
                    .await?;
            }
            synced.push(message);
        } else {
            synced.push(channel_id.say(ctx.http(), content).await?);
        }
    }

    for message in existing_iter {
        message.delete(ctx.http()).await?;
    }

    Ok(synced)
}

fn split_with_header(header: &str, body: String) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for section in split_sections(&body) {
        let section_len = section.chars().count();
        let current_len = current.chars().count();
        let separator_len = if current.is_empty() { 0 } else { 2 };

        if current_len + separator_len + section_len > MAX_MESSAGE_LENGTH {
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
        }

        if !current.is_empty() {
            current.push_str("\n\n");
        }

        current.push_str(&section);
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    let total = chunks.len();

    chunks
        .into_iter()
        .enumerate()
        .map(|(idx, chunk)| {
            if idx == 0 {
                let mut message = String::from(header);
                if total > 1 {
                    use std::fmt::Write;
                    write!(&mut message, " ({}/{total})", idx + 1).expect("write to string");
                }
                message.push_str("\n\n");
                message.push_str(&chunk);
                message
            } else {
                chunk
            }
        })
        .collect()
}

fn split_sections(body: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut current = String::new();

    for line in body.lines() {
        if line.starts_with("## ") && !current.is_empty() {
            push_section(&mut sections, &mut current);
        }

        if !current.is_empty() {
            current.push('\n');
        }

        current.push_str(line);
    }

    push_section(&mut sections, &mut current);

    sections
}

fn push_section(sections: &mut Vec<String>, current: &mut String) {
    if current.is_empty() {
        return;
    }

    while current.ends_with('\n') {
        current.pop();
    }

    sections.push(std::mem::take(current));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repeat_text(text: &str, count: usize) -> String {
        std::iter::repeat(text)
            .take(count)
            .collect::<Vec<_>>()
            .join("")
    }

    #[test]
    fn splits_on_section_boundaries() {
        let body = format!(
            "intro\n\n## Section A\n{}\n\n## Section B\n{}",
            repeat_text("content ", 150),
            repeat_text("more ", 150),
        );

        let messages = split_with_header("HEADER", body);

        assert_eq!(messages.len(), 2);
        assert!(messages[0].contains("## Section A"));
        assert!(!messages[0].contains("## Section B"));
        assert!(messages[1].contains("## Section B"));
        assert!(messages[0].starts_with("HEADER (1/2)\n\n"));
        assert!(!messages[1].starts_with("HEADER"));
    }

    #[test]
    fn keeps_preamble_with_first_section() {
        let body = "intro line\n\n## Section\ncontent".to_string();

        let messages = split_with_header("HEADER", body);

        assert_eq!(messages.len(), 1);
        assert!(messages[0].starts_with("HEADER\n\n"));
        assert!(messages[0].contains("intro line\n\n## Section\ncontent"));
    }
}
