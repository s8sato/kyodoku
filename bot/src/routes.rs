use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use serenity::all::{
    ApplicationCommandInteraction, CommandDataOptionValue, CommandOptionType,
    Context as SerenityContext, CreateCommand, CreateCommandOption, CreateInteractionResponse,
    CreateInteractionResponseDefer, CreateInteractionResponseFollowup, InteractionResponseFlags,
};
use tracing::error;

use crate::isbn;
use crate::util;
use crate::BotState;

pub async fn register_commands(http: &serenity::http::Http) -> Result<()> {
    serenity::all::Command::set_global_application_commands(http, |commands| {
        commands
            .create_command(|cmd| build_isbn_command(cmd))
            .create_command(|cmd| build_watch_command(cmd))
    })
    .await?;
    Ok(())
}

pub async fn handle_interaction(
    ctx: &SerenityContext,
    command: &ApplicationCommandInteraction,
    state: Arc<BotState>,
) -> Result<()> {
    command
        .create_response(
            ctx,
            CreateInteractionResponse::Defer(
                CreateInteractionResponseDefer::new().flags(InteractionResponseFlags::EPHEMERAL),
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
                .flags(InteractionResponseFlags::EPHEMERAL),
        )
        .await?;

    Ok(())
}

fn build_isbn_command(mut cmd: CreateCommand) -> CreateCommand {
    cmd = cmd
        .name("isbn")
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
        );
    cmd
}

fn build_watch_command(mut cmd: CreateCommand) -> CreateCommand {
    cmd = cmd
        .name("watch")
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
        ));
    cmd
}

async fn handle_isbn(
    ctx: &SerenityContext,
    command: &ApplicationCommandInteraction,
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
    let voice_id = util::ensure_isbn_voice_channel(ctx, guild_id, &metadata, &state.store).await?;

    Ok(format!(
        "**{}**\nText thread: <#{}>\nVoice channel: <#{}>",
        metadata.display_title(),
        thread_id,
        voice_id
    ))
}

async fn handle_watch(
    ctx: &SerenityContext,
    command: &ApplicationCommandInteraction,
    state: Arc<BotState>,
) -> Result<String> {
    let guild_id = command
        .guild_id
        .context("Command must be used within a guild")?;
    let user_id = command.user.id;
    let Some(option) = command.data.options.first() else {
        return Err(anyhow!("Missing watch subcommand"));
    };

    match option.name.as_str() {
        "add" => {
            let code = require_string_option(&option.options, "code")?;
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
            let code = require_string_option(&option.options, "code")?;
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

fn require_string_option<'a>(
    options: &'a [serenity::all::ApplicationCommandInteractionDataOption],
    name: &str,
) -> Result<&'a str> {
    options
        .iter()
        .find(|opt| opt.name == name)
        .and_then(|opt| match opt.value.as_ref()? {
            CommandDataOptionValue::String(value) => Some(value.as_str()),
            _ => None,
        })
        .context(format!("Missing required option '{name}'"))
}

fn optional_string_option<'a>(
    options: &'a [serenity::all::ApplicationCommandInteractionDataOption],
    name: &str,
) -> Option<&'a str> {
    options.iter().find_map(|opt| match opt.value.as_ref()? {
        CommandDataOptionValue::String(value) if opt.name == name => Some(value.as_str()),
        _ => None,
    })
}
