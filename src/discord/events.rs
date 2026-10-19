//! Gateway events: chat questions, buttons, dismiss reactions and deletions.

use poise::serenity_prelude as serenity;
use tracing::{debug, warn};

use super::buttons::{self, ButtonAction, MAX_REROLLS};
use super::{dismiss_reaction, is_moderator, votes, Asking, BotState, Error, DISMISS_EMOJI};
use crate::intent::{self, Intent};
use crate::ledger::ReplyRecord;
use crate::meal::Meal;

pub async fn handle(
    framework: poise::FrameworkContext<'_, BotState, Error>,
    event: &serenity::FullEvent,
) -> Result<(), Error> {
    let ctx = framework.serenity_context;
    let state = framework.user_data;

    match event {
        serenity::FullEvent::Message { new_message } => on_message(ctx, state, new_message).await,
        serenity::FullEvent::InteractionCreate {
            interaction: serenity::Interaction::Component(component),
        } => on_button(ctx, state, component).await,
        serenity::FullEvent::ReactionAdd { add_reaction } => {
            on_reaction_add(ctx, state, add_reaction).await
        }
        serenity::FullEvent::MessageDelete {
            channel_id,
            deleted_message_id,
            ..
        } => on_message_delete(ctx, state, *channel_id, *deleted_message_id).await,
        serenity::FullEvent::MessageDeleteBulk {
            channel_id,
            multiple_deleted_messages_ids,
            ..
        } => {
            // Keep going on errors: one reply already purged must not stop
            // the others from being cleaned up.
            for message_id in multiple_deleted_messages_ids {
                if let Err(error) = on_message_delete(ctx, state, *channel_id, *message_id).await {
                    debug!(%error, "could not clean up after a bulk delete");
                }
            }
            Ok(())
        }
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

    let asking = Asking {
        user_id: message.author.id.get(),
        channel_id: message.channel_id.get(),
        guild_id: message.guild_id.map(serenity::GuildId::get),
    };
    let answer = state.answer(&intent, asking);
    let mut builder = serenity::CreateMessage::new()
        .content(answer.reply.text)
        .reference_message(message)
        // Ping the asker like a normal reply, and nobody else.
        .allowed_mentions(serenity::CreateAllowedMentions::new().replied_user(true));
    if answer.reply.recommended.is_some() {
        builder = builder.components(buttons::reroll_row(asking.user_id, answer.meal, 0));
    }
    let reply = message.channel_id.send_message(&ctx.http, builder).await?;

    let stored = state.remember_reply(ReplyRecord {
        reply_id: reply.id.get(),
        trigger_id: Some(message.id.get()),
        owner_id: asking.user_id,
    });
    if !stored {
        // The question was deleted while the answer was on its way.
        reply.delete(&ctx.http).await?;
        return Ok(());
    }
    if let Err(error) = reply.react(&ctx.http, dismiss_reaction()).await {
        warn!(%error, "could not add the dismiss reaction");
    }
    Ok(())
}

async fn on_button(
    ctx: &serenity::Context,
    state: &BotState,
    component: &serenity::ComponentInteraction,
) -> Result<(), Error> {
    match ButtonAction::parse(&component.data.custom_id) {
        Some(ButtonAction::Reroll { owner, meal, count }) => {
            reroll(ctx, state, component, owner, meal, count).await
        }
        Some(action) => votes::press(ctx, state, component, action).await,
        None => Ok(()),
    }
}

async fn reroll(
    ctx: &serenity::Context,
    state: &BotState,
    component: &serenity::ComponentInteraction,
    owner: u64,
    meal: Meal,
    count: u32,
) -> Result<(), Error> {
    let response = if component.user.id.get() != owner {
        buttons::private_response("這不是你的籤，自己問一個吧。")
    } else if count >= MAX_REROLLS {
        buttons::private_response("抽到極限了，就從剛剛那些裡面挑吧。")
    } else {
        let asking = Asking {
            user_id: owner,
            channel_id: component.channel_id.get(),
            guild_id: component.guild_id.map(serenity::GuildId::get),
        };
        let answer = state.answer(&Intent::Recommend { meal: Some(meal) }, asking);
        let components = if answer.reply.recommended.is_some() {
            buttons::reroll_row(owner, meal, count + 1)
        } else {
            Vec::new()
        };
        buttons::update_response(answer.reply.text, Some(components))
    };
    component.create_response(&ctx.http, response).await?;
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

    let allowed = owner == Some(user_id.get())
        || is_moderator(
            ctx,
            reaction.guild_id,
            reaction.channel_id,
            user_id,
            reaction.member.as_ref(),
        )
        .await;
    if !allowed {
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
    if let Some(record) = state.message_deleted(message_id.get()) {
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
