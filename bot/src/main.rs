mod isbn;
mod routes;
mod store;
mod util;

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dotenvy::dotenv;
use redis::Client as RedisClient;
use serenity::all::{
    ApplicationId, Client, Context, GatewayIntents, Interaction, Ready, VoiceState,
};
use serenity::prelude::TypeMapKey;
use songbird::SerenityInit;
use tokio::signal;
use tracing::{error, info};
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

        Ok(Self {
            discord_token,
            application_id,
            database_url,
            redis_url,
            reading_session_activation_threshold,
            voice_cleanup_delay_seconds,
        })
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

#[async_trait]
impl serenity::prelude::EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("Logged in as {}", ready.user.name);
        if let Err(err) = routes::register_commands(&ctx.http).await {
            error!("failed to register commands: {err:?}");
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let Some(command) = interaction.as_command() else {
            return;
        };

        let state = {
            let data = ctx.data.read().await;
            data.get::<StateKey>().cloned()
        };

        let Some(state) = state else {
            error!("Bot state missing from context");
            return;
        };

        if let Err(err) = routes::handle_interaction(&ctx, &command, state).await {
            error!("failed to handle command: {err:?}");
        }
    }

    async fn voice_state_update(&self, ctx: Context, old: Option<VoiceState>, new: VoiceState) {
        let state = {
            let data = ctx.data.read().await;
            data.get::<StateKey>().cloned()
        };

        let Some(state) = state else {
            error!("Bot state missing from context");
            return;
        };

        let guild = new
            .guild_id
            .or_else(|| old.as_ref().and_then(|state| state.guild_id));
        let Some(guild_id) = guild else {
            return;
        };

        let old_channel = old.and_then(|v| v.channel_id);
        let new_channel = new.channel_id;

        if let Err(err) =
            util::handle_voice_state_transition(&ctx, state, guild_id, old_channel, new_channel)
                .await
        {
            error!("voice state handling failed: {err:?}");
        }
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
