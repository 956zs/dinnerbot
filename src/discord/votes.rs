//! Running group votes on Discord. Votes live in memory; a restart ends them.
//!
//! Every press and the close take the vote's `gate` before talking to
//! Discord, so message updates arrive in order and a press can never land
//! after the result and hide it.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use poise::serenity_prelude as serenity;
use tracing::warn;

use super::buttons::{self, ButtonAction};
use super::{is_moderator, lock, ActiveVote, BotState, Error, Votes};
use crate::rng::SplitMix64;
use crate::vote::Vote;

type Gate = Arc<tokio::sync::Mutex<()>>;

impl BotState {
    pub(super) fn channel_has_vote(&self, channel_id: u64) -> bool {
        lock(&self.votes).contains_key(&channel_id)
    }

    /// Claims the channel for a new vote. Returns `false` if one is running.
    pub(super) fn reserve_vote(&self, channel_id: u64, vote: Vote) -> bool {
        let mut votes = lock(&self.votes);
        if votes.contains_key(&channel_id) {
            return false;
        }
        votes.insert(
            channel_id,
            ActiveVote {
                vote,
                message_id: None,
                gate: Gate::default(),
            },
        );
        true
    }

    pub(super) fn release_vote(&self, channel_id: u64) {
        lock(&self.votes).remove(&channel_id);
    }
}

/// Posts the vote reserved for `channel_id` and closes it after `minutes`.
pub(super) async fn start(
    ctx: &serenity::Context,
    state: &BotState,
    channel_id: serenity::ChannelId,
    minutes: u64,
) -> Result<(), Error> {
    let rendered = lock(&state.votes)
        .get(&channel_id.get())
        .map(|active| (active.vote.render_open(), buttons::vote_rows(&active.vote)));
    let Some((content, rows)) = rendered else {
        return Err("vote reservation disappeared before posting".into());
    };

    let builder = serenity::CreateMessage::new()
        .content(content)
        .components(rows)
        .allowed_mentions(serenity::CreateAllowedMentions::new());
    let message = match channel_id.send_message(&ctx.http, builder).await {
        Ok(message) => message,
        Err(error) => {
            state.release_vote(channel_id.get());
            return Err(error.into());
        }
    };
    if let Some(active) = lock(&state.votes).get_mut(&channel_id.get()) {
        active.message_id = Some(message.id.get());
    }

    let http = ctx.http.clone();
    let votes = state.votes.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(minutes * 60)).await;
        // Closed early, or the message was deleted.
        let Some(gate) = gate_for(&votes, message.id.get()) else {
            return;
        };
        let _turn = gate.lock().await;
        let Some(text) = close(&votes, message.id.get()) else {
            return;
        };
        let edit = serenity::EditMessage::new()
            .content(text)
            .components(vec![]);
        if let Err(error) = channel_id.edit_message(&http, message.id, edit).await {
            warn!(%error, "could not post the vote result");
        }
    });
    Ok(())
}

fn find_mut(votes: &mut HashMap<u64, ActiveVote>, message_id: u64) -> Option<&mut ActiveVote> {
    votes
        .values_mut()
        .find(|active| active.message_id == Some(message_id))
}

fn gate_for(votes: &Votes, message_id: u64) -> Option<Gate> {
    find_mut(&mut lock(votes), message_id).map(|active| active.gate.clone())
}

/// Removes a vote and renders its result.
fn close(votes: &Votes, message_id: u64) -> Option<String> {
    let active = {
        let mut votes = lock(votes);
        let channel_id = *votes
            .iter()
            .find(|(_, active)| active.message_id == Some(message_id))?
            .0;
        votes.remove(&channel_id)?
    };
    let outcome = active.vote.outcome(&mut SplitMix64::from_entropy());
    Some(active.vote.render_closed(outcome))
}

/// Handles a press on a vote button.
pub(super) async fn press(
    ctx: &serenity::Context,
    state: &BotState,
    component: &serenity::ComponentInteraction,
    action: ButtonAction,
) -> Result<(), Error> {
    let message_id = component.message.id.get();
    let user_id = component.user.id;

    let Some(gate) = gate_for(&state.votes, message_id) else {
        component
            .create_response(&ctx.http, ended(component))
            .await?;
        return Ok(());
    };
    let _turn = gate.lock().await;

    let response = match action {
        ButtonAction::Vote(index) => {
            let text = find_mut(&mut lock(&state.votes), message_id).map(|active| {
                active.vote.cast(user_id.get(), index);
                active.vote.render_open()
            });
            match text {
                Some(text) => buttons::update_response(text, None),
                None => ended(component),
            }
        }
        ButtonAction::EndVote => {
            let initiator =
                find_mut(&mut lock(&state.votes), message_id).map(|active| active.vote.initiator());
            match initiator {
                None => ended(component),
                Some(initiator) => {
                    let allowed = initiator == user_id.get()
                        || is_moderator(
                            ctx,
                            component.guild_id,
                            component.channel_id,
                            user_id,
                            component.member.as_ref(),
                        )
                        .await;
                    if !allowed {
                        buttons::private_response("只有發起人或管理員可以提前結束。")
                    } else {
                        match close(&state.votes, message_id) {
                            Some(text) => buttons::update_response(text, Some(vec![])),
                            None => ended(component),
                        }
                    }
                }
            }
        }
        ButtonAction::Reroll { .. } => return Ok(()),
    };
    component.create_response(&ctx.http, response).await?;
    Ok(())
}

/// Response for a press on a vote that is no longer open: drop the buttons,
/// and say so when the message does not already show a result (for example
/// after a restart).
fn ended(component: &serenity::ComponentInteraction) -> serenity::CreateInteractionResponse {
    let content = &component.message.content;
    if content.contains("投票結束") {
        let message = serenity::CreateInteractionResponseMessage::new().components(vec![]);
        serenity::CreateInteractionResponse::UpdateMessage(message)
    } else {
        buttons::update_response(
            format!("{content}\n（這個揪團已經失效了，要投票請重新開一個。）"),
            Some(vec![]),
        )
    }
}
