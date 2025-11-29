# Server Configuration Guide

This guide describes how to organize Discord channels for kyodoku so that members can quickly discover active sessions and join the right space.

## Channel order and purpose
Display channels in the following order to keep navigation consistent:

1. **Landing page and command room** — read-only channel that explains how to use kyodoku and accepts slash commands.
2. **Per-ISBN voice channels** — volatile spaces for real-time collaboration.
3. **Per-ISBN text channels** — persistent discussion and handoff space tied to each book.

## Example landing page content
Use the landing page to orient newcomers. A concise template is shown below:

```md
# ようこそ、kyodokuへ

このサーバーでは ISBN ごとに専用の音声・テキストチャンネルを用意しています。

- `/open isbn:<ISBN>` で新しいセッションを作成すると、対応する音声・テキストチャンネルが自動生成されます。
- 音声チャンネルは揮発的で、セッションが終わると消えます。
- テキストチャンネルは永続的で、議事メモやリンクの共有に使えます。

ガイドライン
- 他の参加者がいるか確認してから参加してください。
- 書誌情報は ISBN を使って一意にしてください。
- 参加後は `/watch` でセッション通知をオンにできます。
```

## Enabling direct messages for active session notifications
Kyodoku sends direct messages to notify users about active sessions. Ensure members can receive these DMs:

1. In the Discord client, open **User Settings** → **Privacy & Safety**.
2. Enable **Allow direct messages from server members** for the server that hosts kyodoku.
3. Confirm the kyodoku application has permission to send direct messages (no blocking or privacy overrides).
4. Encourage members to leave DND or focused modes to avoid missing alerts.

With DM permissions enabled and the channel structure above, members can quickly find the right ISBN channel and stay informed about active sessions.
