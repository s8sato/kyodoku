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
# Welcome to the Shared Reading Server :books:
A place where people reading the same book can loosely connect.

## :blue_book: Create a Reading Space: `/open <ISBN>`
- Generates a **voice channel (ephemeral)** and  
  a **text channel (persistent)** for the specified book
- If the channels already exist, you will be guided to them
- The voice channel automatically disappears after a period of inactivity (0 participants)

## :eye_in_speech_bubble: Watchlist & Notifications
### `/watch list`
- Shows your current watchlist  
- When a watched book’s voice channel becomes active, you will receive a **DM notification**
### `/watch add <ISBN1> <ISBN2> ...`
- Adds one or more books to your watchlist
### `/watch remove <ISBN1> <ISBN2> ...`
- Removes one or more books from your watchlist

## :bell: Enabling DM Notifications
To receive active session alerts from the kyodoku app, please enable DMs in Discord:
**User Settings → Privacy & Safety → "Allow direct messages from server members"**
For per-server settings:  
Right-click server name → **Privacy Settings → Allow DMs**

-# All interactions are done through slash commands. Take your time—and enjoy reading at your own pace.

```
