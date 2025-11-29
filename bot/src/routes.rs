use std::collections::HashSet;
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
                "Add one or more ISBNs to your watchlist",
            )
            .add_sub_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "codes",
                    "ISBN-10 or ISBN-13 values (separate with spaces or commas)",
                )
                .required(true),
            ),
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "remove",
                "Remove one or more ISBNs from your watchlist",
            )
            .add_sub_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "codes",
                    "ISBN-10 or ISBN-13 values (separate with spaces or commas)",
                )
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
            let codes = parse_codes(require_string_option(sub_options, "codes")?);
            if codes.is_empty() {
                return Err(anyhow!("At least one ISBN code is required"));
            }

            let mut added = Vec::new();
            let mut seen = HashSet::new();
            let mut errors = Vec::new();

            for code in codes {
                let normalized = match isbn::normalize(code) {
                    Ok(value) => value,
                    Err(err) => {
                        errors.push(format!("{code}: {err}"));
                        continue;
                    }
                };

                if !seen.insert(normalized.isbn_13.clone()) {
                    continue;
                }

                let metadata =
                    match isbn::lookup_metadata(&state.http_client, &normalized, None).await {
                        Ok(value) => value,
                        Err(err) => {
                            errors.push(format!("{}: {err}", normalized.isbn_13));
                            continue;
                        }
                    };

                if let Err(err) = state.store.upsert_isbn(&metadata).await {
                    errors.push(format!("{}: {err}", metadata.isbn_13));
                    continue;
                }

                match state
                    .store
                    .add_watch(guild_id, user_id, &metadata.isbn_13)
                    .await
                {
                    Ok(_) => added.push(format!(
                        "**{}** ({})",
                        metadata.display_title(),
                        metadata.isbn_13
                    )),
                    Err(err) => errors.push(format!("{}: {err}", metadata.isbn_13)),
                }
            }

            let mut responses = Vec::new();
            if !added.is_empty() {
                responses.push(format!("Added {} to your watchlist", added.join(", ")));
            }
            if !errors.is_empty() {
                responses.push(format!(
                    "Some codes could not be processed: {}",
                    errors.join("; ")
                ));
            }

            if responses.is_empty() {
                Ok("No new ISBNs added to your watchlist.".to_string())
            } else {
                Ok(responses.join("\n"))
            }
        }
        "remove" => {
            let codes = parse_codes(require_string_option(sub_options, "codes")?);
            if codes.is_empty() {
                return Err(anyhow!("At least one ISBN code is required"));
            }

            let mut removed = Vec::new();
            let mut seen = HashSet::new();
            let mut errors = Vec::new();

            for code in codes {
                let normalized = match isbn::normalize(code) {
                    Ok(value) => value,
                    Err(err) => {
                        errors.push(format!("{code}: {err}"));
                        continue;
                    }
                };

                if !seen.insert(normalized.isbn_13.clone()) {
                    continue;
                }

                match state
                    .store
                    .remove_watch(guild_id, user_id, &normalized.isbn_13)
                    .await
                {
                    Ok(_) => removed.push(normalized.isbn_13),
                    Err(err) => errors.push(format!("{}: {err}", normalized.isbn_13)),
                }
            }

            let mut responses = Vec::new();
            if !removed.is_empty() {
                responses.push(format!(
                    "Removed {} from your watchlist",
                    removed.join(", ")
                ));
            }
            if !errors.is_empty() {
                responses.push(format!(
                    "Some codes could not be processed: {}",
                    errors.join("; ")
                ));
            }

            if responses.is_empty() {
                Ok("No ISBNs removed from your watchlist.".to_string())
            } else {
                Ok(responses.join("\n"))
            }
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

fn parse_codes(input: &str) -> Vec<&str> {
    input
        .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ';')
        .filter(|part| !part.is_empty())
        .collect()
}
