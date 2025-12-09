# Server Configuration Guide

This guide describes how to organize Discord channels for kyodoku so that members can quickly discover active sessions and join the right space.

## Channel order and purpose

Display channels in the following order to keep navigation consistent:

1. **Landing page and command room** — read-only channel that explains how to use kyodoku and accepts slash commands.
2. **Per-ISBN voice channels** — volatile spaces for real-time collaboration.
3. **Per-ISBN text channels** — persistent discussion and handoff space tied to each book.

## Landing page automation

Use the landing page to orient newcomers. When `COMMAND_INPUT_CHANNEL_ID` is
set, the bot rewrites the first two posts in the command channel (Japanese
first, English second) each time it logs in. The rendered text includes the
current values of `VOICE_CLEANUP_DELAY_SECONDS`, `TEXT_ACTIVITY_EVAL_INTERVAL_SECONDS`,
`TEXT_CATEGORY_SWAP_COUNT`, `TEXT_CATEGORY_PRUNE_COUNT`, and `WATCHLIST_LIMIT`.
Manual edits to those two posts will be overwritten at the next login; adjust
the environment variables instead.
