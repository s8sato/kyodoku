# Railway deployment

Use this guide to deploy **kyodoku** to [Railway](https://railway.app) using the provided [`railway.toml`](../railway.toml) template at the repository root. The bot runs as a worker process built from the existing Dockerfile; it does not expose HTTP endpoints and only needs outbound connectivity to Discord plus private PostgreSQL/Redis instances.

## Prerequisites

1. A Railway account with billing enabled for managed PostgreSQL and Redis plugins.
2. The Railway CLI installed and authenticated (`railway login`).
3. Discord credentials and configuration values for every key in [`bot/.env.example`](../bot/.env.example).
4. A Railway project (create one with `railway init --new kyodoku-bot` or reuse an existing project).

## Prepare services and variables

1. **Create service and plugins**
   - Initialize the project or link to it: `railway init` (choose the target project and environment).
   - Create the bot service that will run the Docker image:
     ```bash
     railway service create kyodoku-bot
     ```
   - Add managed databases in the same environment:
     ```bash
     railway add --plugin postgresql
     railway add --plugin redis
     ```

2. **Bind connection strings**
   - Railway exposes the plugin URLs as variables (e.g., `DATABASE_URL` and `REDIS_URL`). Confirm the names with `railway variables list` and keep them set on the `kyodoku-bot` service.
   - If the variable names differ, set the expected keys explicitly:
     ```bash
     railway variables set DATABASE_URL=${POSTGRES_URL_FROM_PLUGIN}
     railway variables set REDIS_URL=${REDIS_URL_FROM_PLUGIN}
     ```

3. **Set Discord and bot configuration**
   - Populate the remaining keys from [`bot/.env.example`](../bot/.env.example) as service variables. Example:
     ```bash
     railway variables set \
       DISCORD_TOKEN=... \
       APPLICATION_ID=... \
       VOICE_CHANNEL_CATEGORY_ID=... \
      TEXT_CATEGORY_1_ID=... \
      TEXT_CATEGORY_2_ID=... \
      TEXT_CATEGORY_3_ID=... \
      TEXT_CATEGORY_4_ID=... \
      TEXT_CATEGORY_5_ID=... \
      TEXT_CATEGORY_6_ID=... \
      TEXT_CATEGORY_7_ID=... \
      TEXT_CATEGORY_8_ID=... \
      TEXT_CATEGORY_9_ID=... \
      COMMAND_INPUT_CHANNEL_ID=... \
      VOICE_CLEANUP_DELAY_SECONDS=60 \
      TEXT_ACTIVITY_EVAL_INTERVAL_SECONDS=86400 \
      TEXT_CATEGORY_SWAP_COUNT=10 \
      TEXT_CATEGORY_PRUNE_COUNT=10 \
      TIME_ZONE=UTC \
      WATCHLIST_LIMIT=30 \
      --service kyodoku-bot
     ```
   - Adjust optional values as needed; the defaults in [`railway.toml`](../railway.toml) are overridden by these service variables.

## Deploy

1. Ensure the project is linked (`railway link`) and that you are in the repository root.
2. Deploy using the Docker build and Railway config:
   ```bash
   railway up --service kyodoku-bot --detach
   ```
   - The CLI reads [`railway.toml`](../railway.toml) to build from `infra/docker/Dockerfile.bot` and start the worker process.
   - No public ports are required; Railway treats the service as a worker.

## Operations

- **Logs**: `railway logs --service kyodoku-bot --follow`
- **Redeploy**: rerun `railway up --service kyodoku-bot --detach` after updating the code or variables.
- **Secrets rotation**: update values with `railway variables set ...` and redeploy; the container will read the new environment on restart.
- **Scaling**: increase replicas or resources via the Railway dashboard or `railway scale` commands if available for your plan.
