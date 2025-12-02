# Server Configuration Guide

This guide describes how to organize Discord channels for kyodoku so that members can quickly discover active sessions and join the right space.

## Channel order and purpose

Display channels in the following order to keep navigation consistent:

1. **Landing page and command room** — read-only channel that explains how to use kyodoku and accepts slash commands.
2. **Per-ISBN voice channels** — volatile spaces for real-time collaboration.
3. **Per-ISBN text channels** — persistent discussion and handoff space tied to each book.

## Example landing page content

Use the landing page to orient newcomers. A concise template is shown below:

```markdown
# Welcome to the Shared Reading Server :books:
A place where people reading the same book can loosely connect.

## :book: Create a Reading Space
### `/open <ISBN>`
- Generates a **voice channel (ephemeral)** and a **text channel (persistent)** for the specified book
- If the channels already exist, you will be guided to them
- The voice channel automatically disappears after a period of inactivity (0 participants)

## :eye_in_speech_bubble: Watchlist & Notifications
### `/watch list`
- Shows your current watchlist
- When a watched book’s voice channel reaches the participant threshold and becomes active, you will receive a **DM notification**
### `/watch add <ISBN1> <ISBN2> ...`
- Adds one or more books to your watchlist
### `/watch remove <ISBN1> <ISBN2> ...`
- Removes one or more books from your watchlist

## :bell: Enabling DM Notifications
To receive active session alerts from the kyodoku app, please enable DMs in Discord:
**User Settings → Content & Social → DIrect Messages → Allow DMs from other members in this server**
Or:
Right-click server name → **Privacy Settings → Allow DMs from other members in this server**

-# Take your time—and enjoy reading at your own pace.

```

```markdown
# ようこそ、共読サーバーへ :books:
同じ本を読む人がゆるくつながるサーバーです。

## :book: 読書空間の作成
### `/open <ISBN>`
- その本専用の **ボイスチャンネル** と **テキストチャンネル** を生成します
- すでにチャンネルが存在する場合は、案内メッセージを表示します
- ボイスチャンネルは参加者 0人で一定時間が経過すると自動で消えます

## :eye_in_speech_bubble: ウォッチ機能
### `/watch list`
- あなたのウォッチリストを表示します
- ウォッチ中の本のボイスチャンネルがアクティブ（参加者が一定人数に達したとき）になると **DM 通知**が届きます
### `/watch add <ISBN1> <ISBN2> ...`
- ウォッチリストに本を追加します（複数指定可）
### `/watch remove <ISBN1> <ISBN2> ...`
- ウォッチリストから本を削除します（複数指定可）

## :bell: DM 通知を受け取るには
kyodoku アプリからの通知を受信するために、Discord 側で DM を許可してください：
**ユーザー設定 → コンテンツ＆ソーシャル → ダイレクトメッセージ → このサーバーのメンバーからのDMを許可**
または：  
サーバー名を右クリック → **プライバシー設定 → このサーバーのメンバーからのDMを許可**

-# どうぞ、あなたのペースで読書をお楽しみください。

```
