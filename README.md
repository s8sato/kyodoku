# kyodoku

**kyodoku** is a Discord bot that spins up ad-hoc reading rooms for books identified by ISBN. It keeps a persistent text channel
per ISBN, creates temporary voice channels for live discussions, and notifies interested readers when a session becomes active.

The project is maintained with a *spec-first* workflow — refer to [`docs/spec.md`](docs/spec.md) for the authoritative product
requirements.

## Features

- Slash commands for `/open` and `/watch` with metadata lookup via Google Books (primary) and Open Library (fallback).
- Automatic creation/reuse of text channels and voice channels scoped to each ISBN.
- Graceful voice channel cleanup after inactivity and Redis-backed deduplication for watcher notifications.
- PostgreSQL persistence with SQLx migrations.

## Getting Started

1. Copy [`bot/.env.example`](bot/.env.example) to `bot/.env` and fill in your Discord credentials. Set `ALLOWED_GUILD_IDS` to a comma-separated list of server IDs where the bot is permitted to stay (leave it empty to force the bot to exit every guild).
2. Start the development stack:

   ```bash
   docker compose -f infra/docker/docker-compose.yml up --build
   ```

3. Invite the Discord application to your guild with `Send Messages`, `Embed Links`, `Manage Messages`, and `Manage Channels` bot permissions so it can tidy the command intake channel and create the per-ISBN channels used by `/open`, then interact using the `/open` and `/watch` commands.

The bot crate can also be run locally with `cargo run -p kyodoku-bot` once PostgreSQL and Redis are available.

## Repository Layout

```plain
kyodoku/
├─ bot/                # Discord bot implementation (Rust)
│  ├─ migrations/      # SQLx migrations
│  └─ src/             # Bot entrypoint and modules
├─ docs/               # Living specification and architecture docs
├─ infra/docker/       # Local development environment
├─ Cargo.toml          # Workspace manifest
└─ README.md           # Project overview (this file)
```

## License

MIT License © 2025 Shunkichi Sato
