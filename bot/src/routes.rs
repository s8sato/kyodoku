use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use serenity::all::{
    CommandDataOption, CommandDataOptionValue, CommandInteraction, CommandOptionType,
    ComponentInteraction, Context as SerenityContext, CreateActionRow, CreateCommand,
    CreateCommandOption, CreateInteractionResponse, CreateInteractionResponseFollowup,
    CreateInteractionResponseMessage, GuildId, InteractionResponseFlags, MessageFlags,
};
use tracing::debug;

use crate::isbn;
use crate::util;
use crate::BotState;

#[derive(Default)]
pub struct InteractionReply {
    pub content: String,
    pub components: Vec<CreateActionRow>,
}

pub async fn register_commands(http: &serenity::http::Http) -> Result<()> {
    let commands = vec![build_open_command(), build_watch_command()];
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
        "open" => handle_open(ctx, command, state.clone()).await,
        "watch" => handle_watch(ctx, command, state.clone()).await,
        other => Err(anyhow!("Unknown command: {other}")),
    };

    let reply = match response {
        Ok(message) => message,
        Err(err) => {
            debug!("command failed: {err:?}");
            InteractionReply {
                content: format!("❌ {err}"),
                ..Default::default()
            }
        }
    };

    command
        .create_followup(
            &ctx.http,
            CreateInteractionResponseFollowup::new()
                .content(reply.content)
                .components(reply.components)
                .flags(MessageFlags::EPHEMERAL),
        )
        .await?;

    Ok(())
}

fn build_open_command() -> CreateCommand {
    CreateCommand::new("open")
        .description("Create or reuse a reading session for an ISBN")
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "code", "ISBN-10 or ISBN-13")
                .required(true),
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

async fn handle_open(
    ctx: &SerenityContext,
    command: &CommandInteraction,
    state: Arc<BotState>,
) -> Result<InteractionReply> {
    let guild_id = command
        .guild_id
        .context("Command must be used within a guild")?;
    let code = require_string_option(&command.data.options, "code")?;

    let normalized = isbn::normalize(code)?;
    let metadata = isbn::lookup_metadata(&state.http_client, &normalized).await?;

    state.store.upsert_isbn(&metadata).await?;

    let text_ch_id =
        util::ensure_isbn_text_channel(ctx, guild_id, &metadata, state.clone()).await?;
    let voice_ch_id =
        util::ensure_isbn_voice_channel(ctx, guild_id, &metadata, state.clone()).await?;

    Ok(InteractionReply {
        content: format!(
            "Opened a reading session for **{}**(`{}`). Use the buttons below to join.",
            display_title(&metadata.title, metadata.subtitle.as_deref(),),
            metadata.isbn_13,
        ),
        components: util::build_channel_buttons(guild_id, Some(text_ch_id), voice_ch_id, None),
    })
}

pub async fn handle_component_interaction(
    ctx: &SerenityContext,
    component: &ComponentInteraction,
    state: Arc<BotState>,
) -> Result<()> {
    let Some(custom_id) = component
        .data
        .custom_id
        .strip_prefix(util::WATCH_ACTION_PREFIX)
    else {
        return Ok(());
    };

    let mut parts = custom_id.split(':');
    let Some(action) = parts.next() else {
        return Ok(());
    };

    let user_id = component.user.id;

    match action {
        "add" => {
            let Some(isbn_13) = parts.next() else {
                return Ok(());
            };

            let guild_id = component
                .guild_id
                .context("Component interaction must be in a guild")?;

            state.store.add_watch(guild_id, user_id, isbn_13).await?;

            let entry = match state.store.fetch_isbn(isbn_13).await? {
                Some(record) => format_entry(&record.title, record.subtitle.as_deref(), isbn_13),
                None => format_entry(isbn_13, None, isbn_13),
            };

            component
                .create_response(
                    ctx,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content(format!("Added to your watchlist:\n{entry}"))
                            .flags(InteractionResponseFlags::EPHEMERAL),
                    ),
                )
                .await?;
        }
        "remove" => {
            let (guild_id, isbn_13, update_watchlist) = match (parts.next(), parts.next()) {
                (Some(isbn_13), None) => (
                    component
                        .guild_id
                        .context("Component interaction must be in a guild")?,
                    isbn_13,
                    true,
                ),
                (Some(guild_raw), Some(isbn_13)) => {
                    let guild_id = component
                        .guild_id
                        .or_else(|| guild_raw.parse::<u64>().ok().map(GuildId::new))
                        .context("Missing guild for watch remove action")?;

                    (guild_id, isbn_13, false)
                }
                _ => return Ok(()),
            };

            state.store.remove_watch(guild_id, user_id, isbn_13).await?;

            if update_watchlist {
                let mut reply = build_watchlist_reply(&state, guild_id, user_id).await?;
                reply.content = format!(
                    "Removed `{isbn_13}` from your watchlist.\n\n{}",
                    reply.content
                );

                component
                    .create_response(
                        ctx,
                        CreateInteractionResponse::UpdateMessage(
                            CreateInteractionResponseMessage::new()
                                .content(reply.content)
                                .components(reply.components),
                        ),
                    )
                    .await?;
            } else {
                let mut message = CreateInteractionResponseMessage::new()
                    .content(format!("Removed `{isbn_13}` from your watchlist."));

                if component.guild_id.is_some() {
                    message = message.flags(InteractionResponseFlags::EPHEMERAL);
                }

                component
                    .create_response(ctx, CreateInteractionResponse::Message(message))
                    .await?;
            }
        }
        _ => {
            component
                .create_response(
                    ctx,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("Unsupported watch action.")
                            .flags(InteractionResponseFlags::EPHEMERAL),
                    ),
                )
                .await?;
        }
    }

    Ok(())
}

async fn handle_watch(
    _ctx: &SerenityContext,
    command: &CommandInteraction,
    state: Arc<BotState>,
) -> Result<InteractionReply> {
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
            let mut existing_watches: HashSet<String> = state
                .store
                .list_watches(guild_id, user_id)
                .await?
                .into_iter()
                .collect();
            let mut remaining_slots = state
                .config
                .watchlist_limit
                .saturating_sub(existing_watches.len());

            for code in codes {
                let normalized = isbn::normalize(code)?;
                if !seen.insert(normalized.isbn_13.clone()) {
                    continue;
                }

                if existing_watches.contains(&normalized.isbn_13) {
                    continue;
                }

                if remaining_slots == 0 {
                    return Err(anyhow!(format!(
                        "ウォッチリスト登録数が上限（{}冊）に達しました",
                        state.config.watchlist_limit
                    )));
                }

                let metadata = isbn::lookup_metadata(&state.http_client, &normalized).await?;
                state.store.upsert_isbn(&metadata).await?;
                state
                    .store
                    .add_watch(guild_id, user_id, &metadata.isbn_13)
                    .await?;

                existing_watches.insert(metadata.isbn_13.clone());
                remaining_slots = remaining_slots.saturating_sub(1);

                added.push(format_entry(
                    &metadata.title,
                    metadata.subtitle.as_deref(),
                    &metadata.isbn_13,
                ));
            }

            if added.is_empty() {
                Ok(InteractionReply {
                    content: "No new ISBNs added to your watchlist.".to_string(),
                    ..Default::default()
                })
            } else {
                Ok(InteractionReply {
                    content: format!("Added to your watchlist:\n{}", added.join("\n")),
                    ..Default::default()
                })
            }
        }
        "remove" => {
            let codes = parse_codes(require_string_option(sub_options, "codes")?);
            if codes.is_empty() {
                return Err(anyhow!("At least one ISBN code is required"));
            }

            let mut removed = Vec::new();
            let mut seen = HashSet::new();

            for code in codes {
                let normalized = isbn::normalize(code)?;
                if !seen.insert(normalized.isbn_13.clone()) {
                    continue;
                }

                let entry = match state.store.fetch_isbn(&normalized.isbn_13).await? {
                    Some(record) => {
                        format_entry(&record.title, record.subtitle.as_deref(), &record.isbn_13)
                    }
                    None => format_entry(&normalized.isbn_13, None, &normalized.isbn_13),
                };

                state
                    .store
                    .remove_watch(guild_id, user_id, &normalized.isbn_13)
                    .await?;

                removed.push(entry);
            }

            let content = if removed.is_empty() {
                "No ISBNs removed from your watchlist.".to_string()
            } else {
                format!("Removed from your watchlist:\n{}", removed.join("\n"))
            };

            Ok(InteractionReply {
                content,
                ..Default::default()
            })
        }
        "list" => build_watchlist_reply(&state, guild_id, user_id).await,
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

fn parse_codes(input: &str) -> Vec<&str> {
    input
        .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ';')
        .filter(|part| !part.is_empty())
        .collect()
}

fn display_title(title: &str, subtitle: Option<&str>) -> String {
    match subtitle {
        Some(subtitle) if !subtitle.is_empty() => format!("{}: {}", title, subtitle),
        _ => title.to_string(),
    }
}

fn format_entry(title: &str, subtitle: Option<&str>, isbn_13: &str) -> String {
    format!("- `{}` **{}**", isbn_13, display_title(title, subtitle))
}

async fn build_watchlist_reply(
    state: &BotState,
    guild_id: serenity::model::id::GuildId,
    user_id: serenity::model::id::UserId,
) -> Result<InteractionReply> {
    let watches = state.store.list_watches(guild_id, user_id).await?;
    if watches.is_empty() {
        return Ok(InteractionReply {
            content: "Your watchlist is empty.".to_string(),
            ..Default::default()
        });
    }

    let mut entries = Vec::new();
    for isbn_13 in watches {
        let entry = match state.store.fetch_isbn(&isbn_13).await? {
            Some(record) => {
                format_entry(&record.title, record.subtitle.as_deref(), &record.isbn_13)
            }
            None => format_entry(&isbn_13, None, &isbn_13),
        };

        entries.push(entry);
    }

    Ok(InteractionReply {
        content: format!("You are watching:\n{}", entries.join("\n")),
        components: Vec::new(),
    })
}
