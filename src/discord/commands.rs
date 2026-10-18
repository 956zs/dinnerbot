//! Slash commands. They reuse the chat parser's validation, so nothing a
//! user types can make the bot echo mentions or markup.

use poise::serenity_prelude as serenity;
use poise::CreateReply;
use tracing::{error, warn};

use super::{dismiss_reaction, BotState, Context, Error};
use crate::intent::Intent;
use crate::ledger::ReplyRecord;
use crate::meal::Meal;
use crate::text::{self, MAX_OPTIONS};

pub fn all() -> Vec<poise::Command<BotState, Error>> {
    vec![dinner(), choose()]
}

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
pub enum MealChoice {
    #[name = "Breakfast"]
    #[name_localized("zh-TW", "早餐")]
    Breakfast,
    #[name = "Lunch"]
    #[name_localized("zh-TW", "午餐")]
    Lunch,
    #[name = "Dinner"]
    #[name_localized("zh-TW", "晚餐")]
    Dinner,
    #[name = "Late night"]
    #[name_localized("zh-TW", "宵夜")]
    LateNight,
}

impl From<MealChoice> for Meal {
    fn from(choice: MealChoice) -> Self {
        match choice {
            MealChoice::Breakfast => Meal::Breakfast,
            MealChoice::Lunch => Meal::Lunch,
            MealChoice::Dinner => Meal::Dinner,
            MealChoice::LateNight => Meal::LateNight,
        }
    }
}

/// Ask the 8-ball what to eat, or whether a food is a good idea
#[poise::command(
    slash_command,
    guild_only,
    name_localized("zh-TW", "吃什麼"),
    description_localized("zh-TW", "問 8-Ball 這餐吃什麼，或某樣食物適不適合")
)]
pub async fn dinner(
    ctx: Context<'_>,
    #[description = "A food to ask about; leave empty for a suggestion"]
    #[name_localized("zh-TW", "食物")]
    #[description_localized("zh-TW", "想問的食物，不填就直接幫你挑")]
    #[max_length = 64]
    food: Option<String>,
    #[description = "Which meal; defaults to the current time"]
    #[name_localized("zh-TW", "餐別")]
    #[description_localized("zh-TW", "哪一餐，不填就看現在幾點")]
    meal: Option<MealChoice>,
) -> Result<(), Error> {
    let meal = meal.map(Meal::from);
    // Split before cleaning so "火鍋 拉麵" becomes a choice, not one food.
    let options = text::parse_options(food.as_deref().unwrap_or_default());
    let intent = match options {
        // Nothing, "什麼" or "隨便": just pick something.
        Ok(options) if options.is_empty() => Intent::Recommend { meal },
        Ok(mut options) if options.len() == 1 => Intent::Judge {
            food: options.remove(0),
            meal,
        },
        Ok(options) if options.len() <= MAX_OPTIONS => Intent::Choose { options, meal },
        _ => {
            return reply_privately(ctx, "這個我判斷不了，換個食物名稱試試。只能用文字和數字。")
                .await
        }
    };
    send_answer(ctx, &intent).await
}

/// Let the 8-ball pick one of your options
#[poise::command(
    slash_command,
    guild_only,
    name_localized("zh-TW", "幫我選"),
    description_localized("zh-TW", "讓 8-Ball 從你給的選項裡挑一個")
)]
pub async fn choose(
    ctx: Context<'_>,
    #[description = "Options separated by spaces, commas or 還是"]
    #[name_localized("zh-TW", "選項")]
    #[description_localized("zh-TW", "用空白、逗號或「還是」分隔，例如：火鍋 拉麵 壽司")]
    #[max_length = 200]
    options: String,
) -> Result<(), Error> {
    match text::parse_options(&options) {
        Ok(options) if (2..=MAX_OPTIONS).contains(&options.len()) => {
            send_answer(
                ctx,
                &Intent::Choose {
                    options,
                    meal: None,
                },
            )
            .await
        }
        Ok(options) if options.len() < 2 => {
            reply_privately(ctx, "至少給我兩個不一樣的選項。").await
        }
        Ok(_) => reply_privately(ctx, &format!("選項太多了，最多 {MAX_OPTIONS} 個。")).await,
        Err(_) => {
            reply_privately(
                ctx,
                "有選項我判斷不了，只能用文字和數字，不能有 @、連結或符號。",
            )
            .await
        }
    }
}

async fn send_answer(ctx: Context<'_>, intent: &Intent) -> Result<(), Error> {
    let state = ctx.data();
    let user_id = ctx.author().id;

    if let Err(remaining) = state.check_cooldown(user_id.get()) {
        let seconds = remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0);
        return reply_privately(ctx, &format!("慢一點，{seconds} 秒後再問我。")).await;
    }

    let text = state.answer(intent, user_id.get(), ctx.channel_id().get());
    let handle = ctx
        .send(
            CreateReply::default()
                .content(text)
                .allowed_mentions(serenity::CreateAllowedMentions::new()),
        )
        .await?;

    match handle.message().await {
        Ok(message) => {
            state.remember_reply(ReplyRecord {
                reply_id: message.id.get(),
                trigger_id: None,
                owner_id: user_id.get(),
            });
            if let Err(error) = message.react(ctx.http(), dismiss_reaction()).await {
                warn!(%error, "could not add the dismiss reaction");
            }
        }
        Err(error) => warn!(%error, "could not load the command reply"),
    }
    Ok(())
}

async fn reply_privately(ctx: Context<'_>, text: &str) -> Result<(), Error> {
    ctx.send(CreateReply::default().content(text).ephemeral(true))
        .await?;
    Ok(())
}

/// Logs failures; users only ever see a generic message, never internals.
pub async fn on_error(error: poise::FrameworkError<'_, BotState, Error>) {
    match error {
        poise::FrameworkError::Command { error, ctx, .. } => {
            error!(command = %ctx.command().name, %error, "command failed");
            if let Err(send_error) = reply_privately(ctx, "出了點問題，等一下再試。").await
            {
                warn!(error = %send_error, "could not report the command failure");
            }
        }
        poise::FrameworkError::EventHandler { error, event, .. } => {
            error!(event = event.snake_case_name(), %error, "event handler failed");
        }
        poise::FrameworkError::Setup { error, .. } => {
            error!(%error, "startup failed");
        }
        other => {
            if let Err(handler_error) = poise::builtins::on_error(other).await {
                error!(error = %handler_error, "could not handle a framework error");
            }
        }
    }
}
