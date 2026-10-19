//! Button IDs. Everything a button needs is encoded in its ID, so presses
//! keep working after a restart (except votes, which live in memory).

use poise::serenity_prelude as serenity;

use crate::meal::Meal;
use crate::vote::Vote;

/// How many times one recommendation can be re-rolled.
pub const MAX_REROLLS: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonAction {
    /// "再抽一次" on a recommendation.
    Reroll { owner: u64, meal: Meal, count: u32 },
    /// A vote for option `index`.
    Vote(usize),
    /// The initiator ends a vote early.
    EndVote,
}

impl ButtonAction {
    pub fn custom_id(self) -> String {
        match self {
            ButtonAction::Reroll { owner, meal, count } => {
                format!("reroll:{owner}:{}:{count}", meal.code())
            }
            ButtonAction::Vote(index) => format!("vote:{index}"),
            ButtonAction::EndVote => "vote:end".to_owned(),
        }
    }

    pub fn parse(custom_id: &str) -> Option<Self> {
        let mut parts = custom_id.split(':');
        let action = match (parts.next()?, parts.next()?) {
            ("reroll", owner) => {
                let owner = owner.parse().ok()?;
                let mut meal_code = parts.next()?.chars();
                let meal = Meal::from_code(meal_code.next()?)?;
                if meal_code.next().is_some() {
                    return None;
                }
                let count = parts.next()?.parse().ok()?;
                ButtonAction::Reroll { owner, meal, count }
            }
            ("vote", "end") => ButtonAction::EndVote,
            ("vote", index) => ButtonAction::Vote(index.parse().ok()?),
            _ => return None,
        };
        parts.next().is_none().then_some(action)
    }
}

/// The "再抽一次" button for a recommendation re-rolled `count` times.
pub fn reroll_row(owner: u64, meal: Meal, count: u32) -> Vec<serenity::CreateActionRow> {
    let used_up = count >= MAX_REROLLS;
    let label = if used_up {
        "抽到極限了".to_owned()
    } else {
        format!("再抽一次（剩 {} 次）", MAX_REROLLS - count)
    };
    let button =
        serenity::CreateButton::new(ButtonAction::Reroll { owner, meal, count }.custom_id())
            .label(label)
            .style(serenity::ButtonStyle::Secondary)
            .disabled(used_up);
    vec![serenity::CreateActionRow::Buttons(vec![button])]
}

/// One button per option, plus "結束投票" on its own row.
pub fn vote_rows(vote: &Vote) -> Vec<serenity::CreateActionRow> {
    let options = vote
        .options()
        .iter()
        .enumerate()
        .map(|(index, option)| {
            serenity::CreateButton::new(ButtonAction::Vote(index).custom_id())
                .label(format!("{}. {option}", index + 1))
                .style(serenity::ButtonStyle::Primary)
        })
        .collect();
    let end = serenity::CreateButton::new(ButtonAction::EndVote.custom_id())
        .label("結束投票")
        .style(serenity::ButtonStyle::Danger);
    vec![
        serenity::CreateActionRow::Buttons(options),
        serenity::CreateActionRow::Buttons(vec![end]),
    ]
}

/// Replaces the pressed message's text, and its buttons when given.
pub fn update_response(
    text: String,
    components: Option<Vec<serenity::CreateActionRow>>,
) -> serenity::CreateInteractionResponse {
    let mut message = serenity::CreateInteractionResponseMessage::new()
        .content(text)
        .allowed_mentions(serenity::CreateAllowedMentions::new());
    if let Some(components) = components {
        message = message.components(components);
    }
    serenity::CreateInteractionResponse::UpdateMessage(message)
}

/// A reply only the presser can see.
pub fn private_response(text: &str) -> serenity::CreateInteractionResponse {
    serenity::CreateInteractionResponse::Message(
        serenity::CreateInteractionResponseMessage::new()
            .content(text)
            .ephemeral(true),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_ids_round_trip() {
        for action in [
            ButtonAction::Reroll {
                owner: 123_456_789_012_345_678,
                meal: Meal::LateNight,
                count: 2,
            },
            ButtonAction::Vote(0),
            ButtonAction::Vote(4),
            ButtonAction::EndVote,
        ] {
            assert_eq!(ButtonAction::parse(&action.custom_id()), Some(action));
        }
    }

    #[test]
    fn custom_ids_fit_discord_limit() {
        let id = ButtonAction::Reroll {
            owner: u64::MAX,
            meal: Meal::Breakfast,
            count: u32::MAX,
        }
        .custom_id();
        assert!(id.len() <= 100);
    }

    #[test]
    fn malformed_ids_are_rejected() {
        for id in [
            "",
            "reroll",
            "reroll:abc:d:1",
            "reroll:1:x:1",
            "reroll:1:dd:1",
            "reroll:1:d",
            "reroll:1:d:1:extra",
            "vote",
            "vote:-1",
            "vote:end:now",
            "other:1",
        ] {
            assert_eq!(ButtonAction::parse(id), None, "{id}");
        }
    }
}
