//! Gateway events: chat questions, dismiss reactions and deletions.

use poise::serenity_prelude as serenity;
use tracing::{debug, warn};

use super::{dismiss_reaction, BotState, Error, DISMISS_EMOJI};
use crate::intent;
use crate::ledger::ReplyRecord;

pub async fn handle(
    framework: poise::FrameworkContext<'_, BotState, Error>,
    event: &serenity::FullEvent,
) -> Result<(), Error> {
    let ctx = framework.serenity_context;
    let state = framework.user_data;

    match event {
        serenity::FullEvent::Message { new_message } => on_message(ctx, state, new_message).await,
        serenity::FullEvent::ReactionAdd { add_reaction } => {
            on_reaction_add(ctx, state, add_reaction).await
        }
        serenity::FullEvent::MessageDelete {
            channel_id,
            deleted_message_id,
            ..
        } => on_message_delete(ctx, state, *channel_id, *deleted_message_id).await,
        _ => Ok(()),
    }
}

async fn on_message(
    ctx: &serenity::Context,
    state: &BotState,
    message: &serenity::Message,
) -> Result<(), Error> {
    let is_chat = matches!(
        message.kind,
        serenity::MessageType::Regular | serenity::MessageType::InlineReply
    );
    if message.author.bot || message.webhook_id.is_some() || message.guild_id.is_none() || !is_chat
    {
        return Ok(());
    }

    let Some(intent) = intent::parse(&message.content) else {
        return Ok(());
    };
    if state.check_cooldown(message.author.id.get()).is_err() {
        debug!(user = %message.author.id, "question ignored: cooldown");
        return Ok(());
    }

    let text = state.answer(&intent, message.author.id.get(), message.channel_id.get());
    let builder = serenity::CreateMessage::new()
        .content(text)
        .reference_message(message)
        // Ping the asker like a normal reply, and nobody else.
        .allowed_mentions(serenity::CreateAllowedMentions::new().replied_user(true));
    let reply = message.channel_id.send_message(&ctx.http, builder).await?;

    state.remember_reply(ReplyRecord {
        reply_id: reply.id.get(),
        trigger_id: Some(message.id.get()),
        owner_id: message.author.id.get(),
    });
    if let Err(error) = reply.react(&ctx.http, dismiss_reaction()).await {
        warn!(%error, "could not add the dismiss reaction");
    }
    Ok(())
}

async fn on_reaction_add(
    ctx: &serenity::Context,
    state: &BotState,
    reaction: &serenity::Reaction,
) -> Result<(), Error> {
    if !reaction.emoji.unicode_eq(DISMISS_EMOJI) {
        return Ok(());
    }
    let Some(user_id) = reaction.user_id else {
        return Ok(());
    };
    let bot_id = ctx.cache.current_user().id;
    if user_id == bot_id
        || reaction
            .member
            .as_ref()
            .is_some_and(|member| member.user.bot)
    {
        return Ok(());
    }
    // Discord says who wrote the message, so other messages cost no request.
    if reaction
        .message_author_id
        .is_some_and(|author| author != bot_id)
    {
        return Ok(());
    }

    let owner = match state.reply_owner(reaction.message_id.get()) {
        Some(owner) => Some(owner),
        None => {
            // Not in memory, e.g. sent before a restart: ask Discord.
            let message = ctx
                .http
                .get_message(reaction.channel_id, reaction.message_id)
                .await?;
            if message.author.id != bot_id {
                return Ok(());
            }
            asker_of(&message)
        }
    };

    if owner != Some(user_id.get()) && !is_moderator(ctx, reaction, user_id).await {
        return Ok(());
    }

    reaction
        .channel_id
        .delete_message(&ctx.http, reaction.message_id)
        .await?;
    state.forget_reply(reaction.message_id.get());
    Ok(())
}

async fn on_message_delete(
    ctx: &serenity::Context,
    state: &BotState,
    channel_id: serenity::ChannelId,
    message_id: serenity::MessageId,
) -> Result<(), Error> {
    // The question is gone, so its answer goes too.
    if let Some(record) = state.reply_to_deleted(message_id.get()) {
        channel_id
            .delete_message(&ctx.http, serenity::MessageId::new(record.reply_id))
            .await?;
    }
    Ok(())
}

/// Who asked the question a bot message answers.
fn asker_of(message: &serenity::Message) -> Option<u64> {
    if let Some(question) = &message.referenced_message {
        return Some(question.author.id.get());
    }
    match message.interaction_metadata.as_deref() {
        Some(serenity::MessageInteractionMetadata::Command(command)) => Some(command.user.id.get()),
        _ => None,
    }
}

/// Server admins and anyone who can manage messages may dismiss any reply.
async fn is_moderator(
    ctx: &serenity::Context,
    reaction: &serenity::Reaction,
    user_id: serenity::UserId,
) -> bool {
    let Some(guild_id) = reaction.guild_id else {
        return false;
    };

    let fetched;
    let member = match &reaction.member {
        Some(member) => member,
        None => match guild_id.member(ctx, user_id).await {
            Ok(member) => {
                fetched = member;
                &fetched
            }
            Err(error) => {
                warn!(%error, "could not load member to check permissions");
                return false;
            }
        },
    };

    let Some(guild) = ctx.cache.guild(guild_id) else {
        return false;
    };
    let permissions = guild.member_permissions(member);
    permissions.administrator() || permissions.manage_messages()
}
