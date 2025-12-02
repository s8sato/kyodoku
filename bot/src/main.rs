mod archive;
mod isbn;
mod routes;
mod store;
mod util;

use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use dotenvy::dotenv;
use redis::Client as RedisClient;
use serenity::all::{
    ApplicationId, ChannelId, Client, Context, GatewayIntents, Interaction, Message, MessageType,
    Ready, VoiceState,
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
    pub voice_cleanup_delay_seconds: u64,
    pub archive_poll_interval_seconds: u64,
    pub archive_grace_period_seconds: u64,
    pub command_input_channel_id: Option<ChannelId>,
    pub text_channel_category_id: ChannelId,
    pub voice_channel_category_id: ChannelId,
    pub archived_channel_category_id: ChannelId,
    pub text_channel_capacity: usize,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let discord_token = std::env::var("DISCORD_TOKEN")?;
        let application_id = std::env::var("APPLICATION_ID")?.parse()?;
        let database_url = std::env::var("DATABASE_URL")?;
        let redis_url = std::env::var("REDIS_URL")?;
        let voice_cleanup_delay_seconds = std::env::var("VOICE_CLEANUP_DELAY_SECONDS")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value| *value > 0)
            .unwrap_or(60);
        let archive_poll_interval_seconds = std::env::var("ARCHIVE_POLL_INTERVAL_SECONDS")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value| *value > 0)
            .unwrap_or(86_400);
        let archive_grace_period_seconds = std::env::var("ARCHIVE_GRACE_PERIOD_SECONDS")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value| *value > 0)
            .unwrap_or(5_184_000);
        let text_channel_capacity = std::env::var("TEXT_CHANNEL_CAPACITY")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value| *value > 0)
            .unwrap_or(150);
        let command_input_channel_id =
            Self::channel_id_from_env("COMMAND_INPUT_CHANNEL_ID", "command input")?;
        let text_channel_category_id =
            Self::required_channel_id_from_env("TEXT_CHANNEL_CATEGORY_ID", "text")?;
        let voice_channel_category_id =
            Self::required_channel_id_from_env("VOICE_CHANNEL_CATEGORY_ID", "voice")?;
        let archived_channel_category_id =
            Self::required_channel_id_from_env("ARCHIVED_CHANNEL_CATEGORY_ID", "archived")?;

        Ok(Self {
            discord_token,
            application_id,
            database_url,
            redis_url,
            voice_cleanup_delay_seconds,
            archive_poll_interval_seconds,
            archive_grace_period_seconds,
            command_input_channel_id,
            text_channel_category_id,
            voice_channel_category_id,
            archived_channel_category_id,
            text_channel_capacity,
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

    fn required_channel_id_from_env(name: &str, label: &str) -> Result<ChannelId> {
        let Some(id) = Self::channel_id_from_env(name, label)? else {
            return Err(anyhow!("Missing required {label} category id in {name}"));
        };

        Ok(id)
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
}

#[async_trait]
impl serenity::prelude::EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("Logged in as {}", ready.user.name);
        if let Err(err) = routes::register_commands(&ctx.http).await {
            error!("failed to register commands: {err:?}");
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let Some(state) = Handler::state(&ctx).await else {
            error!("Bot state missing from context");
            return;
        };

        if let Some(command) = interaction.as_command() {
            if let Err(err) = routes::handle_interaction(&ctx, command, state).await {
                error!("failed to handle command: {err:?}");
            }
        } else if let Some(component) = interaction.as_message_component() {
            if let Err(err) = routes::handle_component_interaction(&ctx, component, state).await {
                error!("failed to handle component: {err:?}");
            }
        }
    }

    async fn message(&self, ctx: Context, message: Message) {
        let Some(state) = Handler::state(&ctx).await else {
            error!("Bot state missing from context");
            return;
        };

        let Some(channel_id) = state.config.command_input_channel_id else {
            return;
        };

        if message.channel_id != channel_id {
            return;
        }

        if message.author.bot || message.webhook_id.is_some() {
            return;
        }

        if matches!(
            message.kind,
            MessageType::ChatInputCommand | MessageType::ContextMenuCommand
        ) {
            return;
        }

        let is_admin = if let Some(guild_id) = message.guild_id {
            match guild_id.member(&ctx.http, message.author.id).await {
                Ok(member) => match member.permissions(&ctx.cache) {
                    Ok(permissions) => permissions.administrator(),
                    Err(err) => {
                        warn!(
                            "failed to compute permissions for member {}: {err:?}",
                            message.author.id
                        );
                        false
                    }
                },
                Err(err) => {
                    warn!(
                        "failed to fetch member {} from guild {guild_id}: {err:?}",
                        message.author.id
                    );
                    false
                }
            }
        } else {
            false
        };

        if is_admin {
            return;
        }

        if let Err(err) = message.delete(&ctx.http).await {
            warn!("failed to delete message {}: {err:?}", message.id);
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

    let http = client.http.clone();
    let archive_state = state.clone();
    let archive_handle = tokio::spawn(async move {
        archive::run_archive_loop(http, archive_state).await;
    });

    let shard_manager = client.shard_manager.clone();
    let client_future = tokio::spawn(async move {
        if let Err(err) = client.start().await {
            error!("Client exited with error: {err:?}");
        }
    });

    signal::ctrl_c().await?;
    shard_manager.shutdown_all().await;
    archive_handle.abort();
    if let Err(err) = archive_handle.await {
        warn!("Archive task join error: {err:?}");
    }
    if let Err(err) = client_future.await {
        error!("Client task join error: {err:?}");
    }

    Ok(())
}
