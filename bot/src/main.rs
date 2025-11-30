mod isbn;
mod routes;
mod store;
mod util;

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dotenvy::dotenv;
use redis::Client as RedisClient;
use serenity::all::{
    ApplicationId, ChannelId, Client, Context, GatewayIntents, GuildId, Interaction, Ready,
    VoiceState,
};
use serenity::prelude::TypeMapKey;
use songbird::SerenityInit;
use tokio::signal;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use crate::store::Store;

#[derive(Clone)]
pub struct Config {
    pub discord_token: String,
    pub application_id: u64,
    pub database_url: String,
    pub redis_url: String,
    pub reading_session_activation_threshold: usize,
    pub voice_cleanup_delay_seconds: u64,
    pub text_channel_category_id: Option<ChannelId>,
    pub voice_channel_category_id: Option<ChannelId>,
    pub allowed_guilds: HashSet<GuildId>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let discord_token = std::env::var("DISCORD_TOKEN")?;
        let application_id = std::env::var("APPLICATION_ID")?.parse()?;
        let database_url = std::env::var("DATABASE_URL")?;
        let redis_url = std::env::var("REDIS_URL")?;
        let reading_session_activation_threshold =
            std::env::var("READING_SESSION_ACTIVATION_THRESHOLD")
                .ok()
                .and_then(|value| value.parse().ok())
                .filter(|value| *value > 0)
                .unwrap_or(1);
        let voice_cleanup_delay_seconds = std::env::var("VOICE_CLEANUP_DELAY_SECONDS")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value| *value > 0)
            .unwrap_or(60);
        let text_channel_category_id =
            Self::channel_id_from_env("TEXT_CHANNEL_CATEGORY_ID", "text")?;
        let voice_channel_category_id =
            Self::channel_id_from_env("VOICE_CHANNEL_CATEGORY_ID", "voice")?;
        let allowed_guilds = std::env::var("ALLOWED_GUILD_IDS")
            .unwrap_or_default()
            .split(|ch: char| ch == ',' || ch.is_whitespace())
            .filter_map(|raw| {
                if raw.is_empty() {
                    return None;
                }

                match raw.parse::<u64>() {
                    Ok(id) => Some(GuildId::new(id)),
                    Err(err) => {
                        warn!("Ignoring invalid guild id '{raw}': {err:?}");
                        None
                    }
                }
            })
            .collect();

        Ok(Self {
            discord_token,
            application_id,
            database_url,
            redis_url,
            reading_session_activation_threshold,
            voice_cleanup_delay_seconds,
            text_channel_category_id,
            voice_channel_category_id,
            allowed_guilds,
        })
    }

    fn channel_id_from_env(name: &str, label: &str) -> Result<Option<ChannelId>> {
        let raw = match std::env::var(name) {
            Ok(value) if !value.trim().is_empty() => value,
            Ok(_) | Err(std::env::VarError::NotPresent) => return Ok(None),
            Err(err) => return Err(err.into()),
        };

        match raw.parse::<u64>() {
            Ok(id) => Ok(Some(ChannelId::new(id))),
            Err(err) => {
                warn!("Ignoring invalid {label} category id '{raw}': {err:?}");
                Ok(None)
            }
        }
    }

    pub fn is_guild_allowed(&self, guild_id: GuildId) -> bool {
        self.allowed_guilds.contains(&guild_id)
    }
}

#[derive(Clone)]
pub struct BotState {
    pub config: Config,
    pub store: Store,
    pub redis: RedisClient,
    pub http_client: reqwest::Client,
}

struct StateKey;

impl TypeMapKey for StateKey {
    type Value = Arc<BotState>;
}

struct Handler;

impl Handler {
    async fn state(ctx: &Context) -> Option<Arc<BotState>> {
        let data = ctx.data.read().await;
        data.get::<StateKey>().cloned()
    }

    async fn enforce_guild_allowlist(ctx: &Context, state: &BotState, guild_id: GuildId) -> bool {
        if state.config.is_guild_allowed(guild_id) {
            return true;
        }

        info!("Leaving unauthorized guild {}", guild_id);
        if let Err(err) = guild_id.leave(&ctx.http).await {
            error!("failed to leave guild {}: {err:?}", guild_id);
        }

        false
    }
}

#[async_trait]
impl serenity::prelude::EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("Logged in as {}", ready.user.name);
        let Some(state) = Handler::state(&ctx).await else {
            error!("Bot state missing from context");
            return;
        };

        for guild in &ready.guilds {
            Handler::enforce_guild_allowlist(&ctx, &state, guild.id).await;
        }

        if let Err(err) = routes::register_commands(&ctx.http).await {
            error!("failed to register commands: {err:?}");
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let Some(command) = interaction.as_command() else {
            return;
        };

        let Some(state) = Handler::state(&ctx).await else {
            error!("Bot state missing from context");
            return;
        };

        if let Some(guild_id) = command.guild_id {
            if !Handler::enforce_guild_allowlist(&ctx, &state, guild_id).await {
                return;
            }
        }

        if let Err(err) = routes::handle_interaction(&ctx, &command, state).await {
            error!("failed to handle command: {err:?}");
        }
    }

    async fn voice_state_update(&self, ctx: Context, old: Option<VoiceState>, new: VoiceState) {
        let Some(state) = Handler::state(&ctx).await else {
            error!("Bot state missing from context");
            return;
        };

        let guild = new
            .guild_id
            .or_else(|| old.as_ref().and_then(|state| state.guild_id));
        let Some(guild_id) = guild else {
            return;
        };

        if !Handler::enforce_guild_allowlist(&ctx, &state, guild_id).await {
            return;
        }

        let old_channel = old.and_then(|v| v.channel_id);
        let new_channel = new.channel_id;

        if let Err(err) =
            util::handle_voice_state_transition(&ctx, state, guild_id, old_channel, new_channel)
                .await
        {
            error!("voice state handling failed: {err:?}");
        }
    }

    async fn guild_create(
        &self,
        ctx: Context,
        guild: serenity::model::guild::Guild,
        _: Option<bool>,
    ) {
        let Some(state) = Handler::state(&ctx).await else {
            error!("Bot state missing from context");
            return;
        };

        Handler::enforce_guild_allowlist(&ctx, &state, guild.id).await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_env()?;
    let store = Store::connect(&config.database_url).await?;
    let redis = RedisClient::open(config.redis_url.as_str())?;
    let http_client = reqwest::Client::builder()
        .user_agent("kyodoku-bot/0.1")
        .build()?;

    let state = Arc::new(BotState {
        config: config.clone(),
        store,
        redis,
        http_client,
    });

    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_VOICE_STATES
        | GatewayIntents::GUILD_MESSAGES;
    let mut client = Client::builder(&config.discord_token, intents)
        .application_id(ApplicationId::new(config.application_id))
        .event_handler(Handler)
        .register_songbird()
        .await?;

    {
        let mut data = client.data.write().await;
        data.insert::<StateKey>(state.clone());
    }

    let shard_manager = client.shard_manager.clone();
    let client_future = tokio::spawn(async move {
        if let Err(err) = client.start().await {
            error!("Client exited with error: {err:?}");
        }
    });

    signal::ctrl_c().await?;
    shard_manager.shutdown_all().await;
    if let Err(err) = client_future.await {
        error!("Client task join error: {err:?}");
    }

    Ok(())
}
