use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use serenity::all::{
    CommandDataOption, CommandDataOptionValue, CommandInteraction, CommandOptionType,
    Context as SerenityContext, CreateCommand, CreateCommandOption, CreateInteractionResponse,
    CreateInteractionResponseFollowup, CreateInteractionResponseMessage, InteractionResponseFlags,
    MessageFlags,
};
use tracing::error;

use crate::isbn;
use crate::util;
use crate::BotState;

pub async fn register_commands(http: &serenity::http::Http) -> Result<()> {
    let commands = vec![build_isbn_command(), build_watch_command()];
    serenity::all::Command::set_global_commands(http, commands).await?;
    Ok(())
}

pub async fn handle_interaction(
    ctx: &SerenityContext,
    command: &CommandInteraction,
    state: Arc<BotState>,
) -> Result<()> {
    command
        .create_response(
            ctx,
            CreateInteractionResponse::Defer(
                CreateInteractionResponseMessage::new().flags(InteractionResponseFlags::EPHEMERAL),
            ),
        )
        .await?;

    let response = match command.data.name.as_str() {
        "isbn" => handle_isbn(ctx, command, state.clone()).await,
        "watch" => handle_watch(ctx, command, state.clone()).await,
        other => Err(anyhow!("Unknown command: {other}")),
    };

    let content = match response {
        Ok(message) => message,
        Err(err) => {
            error!("command failed: {err:?}");
            format!("❌ {err}")
        }
    };

    command
        .create_followup(
            &ctx.http,
            CreateInteractionResponseFollowup::new()
                .content(content)
                .flags(MessageFlags::EPHEMERAL),
        )
        .await?;

    Ok(())
}

fn build_isbn_command() -> CreateCommand {
    CreateCommand::new("isbn")
        .description("Create or reuse a voice channel for an ISBN")
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "code", "ISBN-10 or ISBN-13")
                .required(true),
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "title_override",
                "Override title if lookup fails",
            )
            .required(false),
        )
}

fn build_watch_command() -> CreateCommand {
    CreateCommand::new("watch")
        .description("Manage ISBN watchlist")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "add",
                "Add an ISBN to your watchlist",
            )
            .add_sub_option(
                CreateCommandOption::new(CommandOptionType::String, "code", "ISBN-10 or ISBN-13")
                    .required(true),
            ),
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "remove",
                "Remove an ISBN from your watchlist",
            )
            .add_sub_option(
                CreateCommandOption::new(CommandOptionType::String, "code", "ISBN-10 or ISBN-13")
                    .required(true),
            ),
        )
        .add_option(CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "list",
            "List watched ISBNs",
        ))
}

async fn handle_isbn(
    ctx: &SerenityContext,
    command: &CommandInteraction,
    state: Arc<BotState>,
) -> Result<String> {
    let guild_id = command
        .guild_id
        .context("Command must be used within a guild")?;
    let code = require_string_option(&command.data.options, "code")?;
    let title_override = optional_string_option(&command.data.options, "title_override");

    let normalized = isbn::normalize(code)?;
    let metadata = isbn::lookup_metadata(&state.http_client, &normalized, title_override).await?;

    state.store.upsert_isbn(&metadata).await?;

    let thread_id = util::ensure_isbn_thread(ctx, guild_id, &metadata, &state.store).await?;
    let voice_id = util::ensure_isbn_voice_channel(ctx, guild_id, &metadata, state.clone()).await?;

    Ok(format!(
        "**{}**\nText thread: <#{}>\nVoice channel: <#{}>",
        metadata.display_title(),
        thread_id,
        voice_id
    ))
}

async fn handle_watch(
    _ctx: &SerenityContext,
    command: &CommandInteraction,
    state: Arc<BotState>,
) -> Result<String> {
    let guild_id = command
        .guild_id
        .context("Command must be used within a guild")?;
    let user_id = command.user.id;
    let Some(option) = command.data.options.first() else {
        return Err(anyhow!("Missing watch subcommand"));
    };
    let CommandDataOptionValue::SubCommand(sub_options) = &option.value else {
        return Err(anyhow!("Unexpected option for watch command"));
    };

    match option.name.as_str() {
        "add" => {
            let code = require_string_option(sub_options, "code")?;
            let normalized = isbn::normalize(code)?;
            let metadata = isbn::lookup_metadata(&state.http_client, &normalized, None).await?;
            state.store.upsert_isbn(&metadata).await?;
            state
                .store
                .add_watch(guild_id, user_id, &metadata.isbn_13)
                .await?;
            Ok(format!(
                "Added **{}** ({}) to your watchlist",
                metadata.display_title(),
                metadata.isbn_13
            ))
        }
        "remove" => {
            let code = require_string_option(sub_options, "code")?;
            let normalized = isbn::normalize(code)?;
            state
                .store
                .remove_watch(guild_id, user_id, &normalized.isbn_13)
                .await?;
            Ok(format!(
                "Removed {} from your watchlist",
                normalized.isbn_13
            ))
        }
        "list" => {
            let watches = state.store.list_watches(guild_id, user_id).await?;
            if watches.is_empty() {
                Ok("Your watchlist is empty.".to_string())
            } else {
                Ok(format!("You are watching: {}", watches.join(", ")))
            }
        }
        other => Err(anyhow!("Unknown watch action: {other}")),
    }
}

fn require_string_option<'a>(options: &'a [CommandDataOption], name: &str) -> Result<&'a str> {
    options
        .iter()
        .find_map(|opt| match (&opt.value, opt.name.as_str()) {
            (CommandDataOptionValue::String(value), option_name) if option_name == name => {
                Some(value.as_str())
            }
            _ => None,
        })
        .context(format!("Missing required option '{name}'"))
}

fn optional_string_option<'a>(options: &'a [CommandDataOption], name: &str) -> Option<&'a str> {
    options
        .iter()
        .find_map(|opt| match (&opt.value, opt.name.as_str()) {
            (CommandDataOptionValue::String(value), option_name) if option_name == name => {
                Some(value.as_str())
            }
            _ => None,
        })
}
