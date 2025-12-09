# Fly.io deployment (Machines)

Use this guide to deploy **kyodoku** to Fly.io using the provided [`infra/fly/fly.toml`](../infra/fly/fly.toml) template and the
existing Docker build. The bot runs as a background worker with no public services; it connects outbound to Discord and the
private PostgreSQL/Redis endpoints you provision.

## Prerequisites

1. A Fly.io account and organization with billing enabled for managed Postgres/Redis add-ons.
2. `flyctl` installed and authenticated (`fly auth login`).
3. Discord credentials and server configuration values for every key in [`bot/.env.example`](../bot/.env.example) (token,
   application ID, category IDs, etc.).
4. Access to a region near your Discord servers (e.g., `iad`, `fra`, `nrt`).

## Prepare the Fly app

1. **Create the app**
   - `fly apps create kyodoku-bot-<suffix>` (choose a unique name).
   - Update `app` and `primary_region` in [`infra/fly/fly.toml`](../infra/fly/fly.toml) to match.

2. **Provision PostgreSQL**
   - `fly postgres create --name kyodoku-postgres-<suffix> --region <region> --vm-size shared-cpu-1x --volume-size 10`
   - Attach it to the bot app: `fly postgres attach kyodoku-postgres-<suffix> -a kyodoku-bot-<suffix>`
   - The attach step injects `DATABASE_URL` as a secret on the bot app.

3. **Provision Redis (Upstash)**
   - `fly redis create --name kyodoku-redis-<suffix> --primary-region <region>`
   - Copy the `UPSTASH_REDIS_URL` from the command output and set it as the bot's Redis endpoint:
     `fly secrets set REDIS_URL=<upstash-url> -a kyodoku-bot-<suffix>`

4. **Set Discord and bot configuration secrets**
   - Populate the remaining variables from `bot/.env.example` via secrets. Example:
     ```bash
     fly secrets set \
     DISCORD_TOKEN=... \
      APPLICATION_ID=... \
      VOICE_CHANNEL_CATEGORY_ID=... \
     # Configure TEXT_CATEGORY_1_ID and continue consecutively (e.g., TEXT_CATEGORY_2_ID ... up to TEXT_CATEGORY_9_ID) \
     TEXT_CATEGORY_1_ID=... \
     TEXT_CATEGORY_2_ID=... \
     TEXT_CATEGORY_3_ID=... \
      COMMAND_INPUT_CHANNEL_ID=... \
      VOICE_CLEANUP_DELAY_SECONDS=60 \
      TEXT_ACTIVITY_EVAL_INTERVAL_SECONDS=86400 \
      TEXT_ACTIVITY_PRESENCE_FACTOR=2.0 \
      TEXT_CATEGORY_SWAP_COUNT=10 \
      TEXT_CATEGORY_PRUNE_COUNT=10 \
      TEXT_CATEGORY_CAPACITY=50 \
      TIME_ZONE=UTC \
      WATCHLIST_LIMIT=30 \
      -a kyodoku-bot-<suffix>
     ```
   - Omit or adjust optional values as needed. Secrets override any defaults defined in the Fly config file.

## Deploy

Run the deployment from the repository root using the Fly config:

```bash
fly deploy -c infra/fly/fly.toml --remote-only --app kyodoku-bot-<suffix>
```

- The build uses `infra/docker/Dockerfile.bot`; no local Docker daemon is required when `--remote-only` is set.
- The process name is `bot`, and no TCP services are exposed. Discord connectivity and database/cache access rely on outbound
  networking.

## Operations

- **Logs**: `fly logs -a kyodoku-bot-<suffix>`
- **Status and releases**: `fly status -a kyodoku-bot-<suffix>` and `fly releases -a kyodoku-bot-<suffix>`
- **Secrets rotation**: run another `fly secrets set ...` command; redeploy is not required.
- **Scaling**: adjust the VM size in [`infra/fly/fly.toml`](../infra/fly/fly.toml) (the `[[vm]]` block) or use `fly scale` to
  change CPU/memory. Keep at least one machine running so the bot can maintain its Discord session.
