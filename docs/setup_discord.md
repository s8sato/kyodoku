# Running the Kyodoku Bot on Discord

This guide walks through preparing credentials, configuring the bot container, and inviting it to your Discord server. Follow the steps in order to try the bot locally with Docker Compose.

## 1. Create a Discord application and bot user
1. Open the [Discord Developer Portal](https://discord.com/developers/applications) and click **New Application**.
2. Name the application (e.g., `kyodoku-local`) and confirm. The portal redirects to the application dashboard.
3. Copy the **Application ID** from the **General Information** page. You will paste it into the `.env` file later as `APPLICATION_ID`.
4. In the left sidebar, navigate to **Bot** → **Add Bot**, then confirm by selecting **Yes, do it!**. The portal creates a bot user for the application.
5. Under the bot settings, click **Reset Token** and then **Copy** to reveal the **Bot Token**. Save it temporarily—you will need it for the `.env` file as `DISCORD_TOKEN`.

> ⚠️ Treat the bot token like a password. Do not commit it to version control or share it publicly. If it leaks, return to this page and regenerate a new token.

## 2. Configure environment variables
1. Duplicate the sample environment file: `cp bot/.env.example bot/.env` from the repository root.
2. Open `bot/.env` in your editor.
3. Fill in the required variables:
   - `DISCORD_TOKEN=` — paste the bot token from step 1.
   - `APPLICATION_ID=` — paste the application ID from step 1.
   - `ALLOWED_GUILD_IDS=` — a comma-separated list of Discord server IDs where the bot is allowed to stay. The bot will immediately leave any guild not listed here. If you leave this empty, the bot will exit every guild, including ones you invite it to.
     - To copy a server ID, enable **Developer Mode** in Discord user settings → **Advanced**, then right-click the server name in the sidebar and choose **Copy Server ID**.
   - `TEXT_CHANNEL_CATEGORY_ID=` — optional. When set, new ISBN text threads are created inside this category.
   - `VOICE_CHANNEL_CATEGORY_ID=` — optional. When set, ISBN voice channels are created inside this category.
   - Leave the PostgreSQL and Redis connection strings as their defaults unless you are running external services.
4. Save the file. The bot container will read these values on startup.

## 3. Launch the local stack
From the repository root, start the Docker Compose stack defined under `infra/docker/`:

```bash
docker compose -f infra/docker/docker-compose.yml up --build
```

The command builds the bot image (if necessary) and launches three containers:
- `kyodoku-postgres` — PostgreSQL for persistence.
- `kyodoku-redis` — Redis for caching and distributed locks.
- `kyodoku-bot` — the Discord bot itself.

Wait until the bot logs show it connected to Discord before proceeding.

## 4. Invite the bot to your server
1. Return to the application in the Developer Portal and open **OAuth2** → **URL Generator**.
2. In **Scopes**, select `bot` and `applications.commands`. The latter is required for slash commands.
3. In **Bot Permissions**, grant only the permissions the bot needs. For testing the sample commands, select both `Send Messages` and `Manage Channels` so the bot can create the text and voice channels used by `/open`.
4. Copy the generated URL, open it in your browser, choose the server where you have the “Manage Server” permission, and authorize the bot.
   - If you previously invited the bot without `Manage Channels`, reinvite it with the updated permissions so slash commands can provision their channels successfully.
5. After authorization, the bot user appears in the server member list.

## 5. Test the commands
1. Confirm the Docker stack is still running and the bot container logs indicate it is ready.
2. In the target Discord server, open a text channel and type `/open` or `/watch`. The Discord client should autocomplete the command if the bot registered correctly.
3. Submit the command to verify the bot responds. If commands are missing, ensure the application was invited with `applications.commands` scope and the bot container restarted after editing `.env`.

## Troubleshooting tips
- **Bot shows offline:** Check the Compose logs for authentication errors. Regenerate the bot token if needed and restart the stack.
- **Commands not available:** Slash commands can take up to an hour to propagate globally. For immediate testing, enable the **Guild Install** option in the application settings and reinvite the bot.
- **Port conflicts:** Stop other local PostgreSQL/Redis containers that bind to the same ports (`5432`, `6379`). Alternatively, edit `infra/docker/docker-compose.yml` to adjust port mappings.

With the bot authorized and responding to slash commands, you are ready to iterate on kyodoku features locally.
