mod activity;
mod isbn;
mod landing;
mod routes;
mod store;
mod util;

use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono_tz::Tz;
use dotenvy::dotenv;
use redis::Client as RedisClient;
use serenity::all::{
    ApplicationId, ChannelId, Client, Context, GatewayIntents, GuildId, Interaction, Message,
    MessageType, Ready, VoiceState,
};
use serenity::prelude::TypeMapKey;
use songbird::SerenityInit;
use tokio::signal;
use tokio::task::JoinError;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use crate::store::Store;

#[derive(Clone)]
pub struct Config {
    pub discord_token: String,
    pub application_id: u64,
    pub home_guild_id: GuildId,
    pub database_url: String,
    pub redis_url: String,
    pub voice_cleanup_delay_seconds: u64,
    pub command_input_channel_id: Option<ChannelId>,
    pub text_category_ids: Vec<ChannelId>,
    pub voice_channel_category_id: ChannelId,
    pub text_activity_eval_interval_seconds: u64,
    pub text_activity_presence_factor: f64,
    pub text_category_swap_count: usize,
    pub text_category_prune_count: usize,
    pub text_category_capacity: usize,
    pub watchlist_limit: usize,
    pub time_zone: Tz,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let discord_token = std::env::var("DISCORD_TOKEN")?;
        let application_id = std::env::var("APPLICATION_ID")?.parse()?;
        let home_guild_id = Self::required_guild_id_from_env("GUILD_ID", "home guild")?;
        let database_url = std::env::var("DATABASE_URL")?;
        let redis_url = std::env::var("REDIS_URL")?;
        let voice_cleanup_delay_seconds = std::env::var("VOICE_CLEANUP_DELAY_SECONDS")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value| *value > 0)
            .unwrap_or(60);
        let text_activity_eval_interval_seconds =
            std::env::var("TEXT_ACTIVITY_EVAL_INTERVAL_SECONDS")
                .ok()
                .and_then(|value| value.parse().ok())
                .filter(|value| *value > 0)
                .unwrap_or(86_400);
        let text_activity_presence_factor = std::env::var("TEXT_ACTIVITY_PRESENCE_FACTOR")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value: &f64| *value > 0.0)
            .unwrap_or(2.0);
        let text_category_swap_count = std::env::var("TEXT_CATEGORY_SWAP_COUNT")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value| *value > 0)
            .unwrap_or(10);
        let text_category_prune_count = std::env::var("TEXT_CATEGORY_PRUNE_COUNT")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value| *value > 0)
            .unwrap_or(10);
        let min_text_category_capacity =
            text_category_swap_count + text_category_swap_count.max(text_category_prune_count);
        let text_category_capacity = std::env::var("TEXT_CATEGORY_CAPACITY")
            .ok()
            .and_then(|value| value.parse().ok())
            .map(|value: usize| value.clamp(min_text_category_capacity, 50))
            .unwrap_or(50);
        let watchlist_limit = std::env::var("WATCHLIST_LIMIT")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value| *value > 0)
            .unwrap_or(30);
        let time_zone = std::env::var("TIME_ZONE")
            .ok()
            .and_then(|value| match value.parse::<Tz>() {
                Ok(tz) => Some(tz),
                Err(err) => {
                    warn!("Ignoring invalid time zone '{value}': {err:?}; defaulting to UTC");
                    None
                }
            })
            .unwrap_or(chrono_tz::UTC);
        let command_input_channel_id =
            Self::channel_id_from_env("COMMAND_INPUT_CHANNEL_ID", "command input")?;
        let mut text_category_ids = Vec::with_capacity(9);
        for idx in 1..=9 {
            let env_key = format!("TEXT_CATEGORY_{idx}_ID");
            match std::env::var(&env_key) {
                Ok(value) if !value.trim().is_empty() => {
                    text_category_ids.push(Self::required_channel_id_from_env(
                        &env_key,
                        &format!("text category {idx}"),
                    )?);
                }
                Ok(_) | Err(std::env::VarError::NotPresent) => break,
                Err(err) => return Err(err.into()),
            }
        }

        if text_category_ids.is_empty() {
            return Err(anyhow!(
                "At least one text category (TEXT_CATEGORY_1_ID) must be configured"
            ));
        }
        let voice_channel_category_id =
            Self::required_channel_id_from_env("VOICE_CHANNEL_CATEGORY_ID", "voice")?;

        Ok(Self {
            discord_token,
            application_id,
            home_guild_id,
            database_url,
            redis_url,
            voice_cleanup_delay_seconds,
            command_input_channel_id,
            text_category_ids,
            voice_channel_category_id,
            text_activity_eval_interval_seconds,
            text_activity_presence_factor,
            text_category_swap_count,
            text_category_prune_count,
            text_category_capacity,
            watchlist_limit,
            time_zone,
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

    fn guild_id_from_env(name: &str, label: &str) -> Result<Option<GuildId>> {
        let raw = match std::env::var(name) {
            Ok(value) if !value.trim().is_empty() => value,
            Ok(_) | Err(std::env::VarError::NotPresent) => return Ok(None),
            Err(err) => return Err(err.into()),
        };

        match raw.parse::<u64>() {
            Ok(id) => Ok(Some(GuildId::new(id))),
            Err(err) => {
                warn!("Ignoring invalid {label} id '{raw}': {err:?}");
                Ok(None)
            }
        }
    }

    fn required_guild_id_from_env(name: &str, label: &str) -> Result<GuildId> {
        let Some(id) = Self::guild_id_from_env(name, label)? else {
            return Err(anyhow!("Missing required {label} id in {name}"));
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

enum ShutdownReason {
    Signal(std::io::Result<()>),
    ClientFinished,
}

#[async_trait]
impl serenity::prelude::EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("Logged in as {}", ready.user.name);
        if let Err(err) = routes::register_commands(&ctx.http).await {
            error!("failed to register commands: {err:?}");
        }

        let Some(state) = Handler::state(&ctx).await else {
            error!("Bot state missing from context");
            return;
        };

        if let Err(err) = landing::refresh_landing_posts(&ctx, &state.config, ready.user.id).await {
            warn!("failed to refresh landing posts: {err:?}");
        }

        let ctx_clone = ctx.clone();
        let state_clone = state.clone();
        tokio::spawn(async move {
            if let Err(err) = activity::run_text_activity_eval_loop(ctx_clone, state_clone).await {
                warn!("failed to evaluate text channel activity: {err:?}");
            }
        });
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

        if message.guild_id != Some(state.config.home_guild_id) {
            return;
        }

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

        if guild_id != state.config.home_guild_id {
            return;
        }

        let old_channel = old.and_then(|v| v.channel_id);
        let new_channel = new.channel_id;

        if let Err(err) = util::handle_voice_state_transition(
            &ctx,
            state,
            guild_id,
            new.user_id,
            old_channel,
            new_channel,
        )
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
    let client_handle = tokio::spawn(async move { client.start().await });
    tokio::pin!(client_handle);

    let mut client_result: Option<Result<Result<(), serenity::Error>, JoinError>> = None;
    let shutdown_reason = tokio::select! {
        result = signal::ctrl_c() => ShutdownReason::Signal(result),
        result = &mut client_handle => {
            client_result = Some(result);
            ShutdownReason::ClientFinished
        },
    };

    shard_manager.shutdown_all().await;

    if client_result.is_none() {
        client_result = Some(client_handle.await);
    }

    let mut exit_code = match (&shutdown_reason, client_result.as_ref()) {
        (ShutdownReason::Signal(Ok(())), _) => {
            info!("Shutdown signal received; stopping bot.");
            130
        }
        (ShutdownReason::Signal(Err(err)), _) => {
            error!("Failed to listen for shutdown signal: {err:?}");
            1
        }
        (ShutdownReason::ClientFinished, Some(Ok(Ok(())))) => {
            warn!("Discord client exited unexpectedly without shutdown signal.");
            1
        }
        (ShutdownReason::ClientFinished, Some(Ok(Err(err)))) => {
            error!("Discord client exited with error: {err:?}");
            1
        }
        (ShutdownReason::ClientFinished, Some(Err(err))) => {
            error!("Discord client task join error: {err:?}");
            1
        }
        (ShutdownReason::ClientFinished, None) => {
            error!("Discord client finished without join result.");
            1
        }
    };

    if let Some(result) = client_result {
        if !matches!(shutdown_reason, ShutdownReason::ClientFinished) {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(err)) => {
                    error!("Discord client exited with error after shutdown: {err:?}");
                    exit_code = exit_code.max(1);
                }
                Err(err) => {
                    error!("Discord client task join error after shutdown: {err:?}");
                    exit_code = exit_code.max(1);
                }
            }
        }
    }

    std::process::exit(exit_code);
}
