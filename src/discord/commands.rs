//! Slash commands. They reuse the chat parser's validation, so nothing a
//! user types can make the bot echo mentions or markup.

use poise::serenity_prelude as serenity;
use poise::CreateReply;
use tracing::{error, warn};

use super::buttons;
use super::{dismiss_reaction, votes, Asking, BotState, Context, Error};
use crate::data::MenuMode;
use crate::intent::Intent;
use crate::ledger::ReplyRecord;
use crate::meal::Meal;
use crate::store::{EditError, MAX_AVOID_KEYWORDS, MAX_KEYWORD_CHARS, MAX_MENU_ITEMS};
use crate::text::{self, MAX_FOOD_CHARS, MAX_OPTIONS};
use crate::vote::{self, Vote};

pub fn all() -> Vec<poise::Command<BotState, Error>> {
    vec![dinner(), choose(), vote(), menu(), avoid(), help()]
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

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
pub enum MenuModeChoice {
    #[name = "Mixed"]
    #[name_localized("zh-TW", "混合")]
    Mixed,
    #[name = "Server menu only"]
    #[name_localized("zh-TW", "只用自訂")]
    CustomOnly,
    #[name = "Built-in menu only"]
    #[name_localized("zh-TW", "只用內建")]
    BuiltinOnly,
}

impl From<MenuModeChoice> for MenuMode {
    fn from(choice: MenuModeChoice) -> Self {
        match choice {
            MenuModeChoice::Mixed => MenuMode::Mixed,
            MenuModeChoice::CustomOnly => MenuMode::CustomOnly,
            MenuModeChoice::BuiltinOnly => MenuMode::BuiltinOnly,
        }
    }
}

fn mode_label(mode: MenuMode) -> &'static str {
    match mode {
        MenuMode::Mixed => "混合（自訂和內建大約各半）",
        MenuMode::CustomOnly => "只用自訂",
        MenuMode::BuiltinOnly => "只用內建",
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
        Err(_) => reply_privately(ctx, OPTIONS_INVALID).await,
    }
}

const ALREADY_VOTING: &str = "這個頻道已經有揪團在投票了，等它結束再開。";

const OPTIONS_INVALID: &str = "有選項我判斷不了，只能用文字和數字，不能有 @、連結或符號。";

/// Start a group vote on where to eat
#[poise::command(
    slash_command,
    guild_only,
    name_localized("zh-TW", "揪團"),
    description_localized("zh-TW", "抽幾個候選讓大家投票決定吃什麼")
)]
pub async fn vote(
    ctx: Context<'_>,
    #[description = "Your own options; leave empty to let the bot pick"]
    #[name_localized("zh-TW", "選項")]
    #[description_localized("zh-TW", "自己給候選，用空白或逗號分隔；不填就由 bot 抽")]
    #[max_length = 200]
    options: Option<String>,
    #[description = "How many candidates the bot picks (2-5)"]
    #[name_localized("zh-TW", "候選數")]
    #[description_localized("zh-TW", "bot 要抽幾個候選（2 到 5）")]
    #[min = 2]
    #[max = 5]
    count: Option<u8>,
    #[description = "Minutes until the vote closes (1-30)"]
    #[name_localized("zh-TW", "分鐘")]
    #[description_localized("zh-TW", "幾分鐘後截止（1 到 30）")]
    #[min = 1]
    #[max = 30]
    minutes: Option<u64>,
    #[description = "Which meal; defaults to the current time"]
    #[name_localized("zh-TW", "餐別")]
    #[description_localized("zh-TW", "哪一餐，不填就看現在幾點")]
    meal: Option<MealChoice>,
) -> Result<(), Error> {
    let state = ctx.data();
    let channel_id = ctx.channel_id();
    if state.channel_has_vote(channel_id.get()) {
        return reply_privately(ctx, ALREADY_VOTING).await;
    }
    if let Err(remaining) = state.check_cooldown(ctx.author().id.get()) {
        return reply_privately(ctx, &cooldown_message(remaining)).await;
    }

    let meal = meal.map_or_else(|| state.current_meal(), Meal::from);
    let minutes = minutes
        .unwrap_or(vote::DEFAULT_MINUTES)
        .clamp(vote::MIN_MINUTES, vote::MAX_MINUTES);
    let candidates = match options.as_deref() {
        Some(raw) => match text::parse_options(raw) {
            Ok(options) if options.len() > vote::MAX_OPTIONS => {
                let text = format!("揪團最多 {} 個選項。", vote::MAX_OPTIONS);
                return reply_privately(ctx, &text).await;
            }
            Ok(options) => options,
            Err(_) => return reply_privately(ctx, OPTIONS_INVALID).await,
        },
        None => {
            let count = count
                .map_or(vote::DEFAULT_OPTIONS, usize::from)
                .clamp(vote::MIN_OPTIONS, vote::MAX_OPTIONS);
            state.vote_candidates(count, meal, asking(ctx))
        }
    };
    if candidates.len() < vote::MIN_OPTIONS {
        return reply_privately(
            ctx,
            "候選不夠兩個，給我至少兩個不一樣的選項，或調整一下忌口。",
        )
        .await;
    }

    // Claim the channel before any await, so two votes cannot both start.
    let vote = Vote::new(candidates, ctx.author().id.get(), meal, minutes);
    if !state.reserve_vote(channel_id.get(), vote) {
        return reply_privately(ctx, ALREADY_VOTING).await;
    }
    // Posting can take a moment; acknowledge first so Discord does not time out.
    if let Err(error) = ctx.defer_ephemeral().await {
        state.release_vote(channel_id.get());
        return Err(error.into());
    }
    votes::start(ctx.serenity_context(), state, channel_id, minutes).await?;
    reply_privately(ctx, "揪團開始了！").await
}

/// Manage this server's own food list
#[poise::command(
    slash_command,
    guild_only,
    name_localized("zh-TW", "菜單"),
    description_localized("zh-TW", "管理這個伺服器自己的店家和食物"),
    subcommands("menu_add", "menu_remove", "menu_list", "menu_mode"),
    subcommand_required
)]
pub async fn menu(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Add a food or restaurant to this server's menu
#[poise::command(
    slash_command,
    guild_only,
    rename = "add",
    name_localized("zh-TW", "新增"),
    description_localized("zh-TW", "把常吃的店或食物加進這個伺服器的菜單")
)]
pub async fn menu_add(
    ctx: Context<'_>,
    #[description = "Food or restaurant name"]
    #[name_localized("zh-TW", "名稱")]
    #[description_localized("zh-TW", "店名或食物名稱")]
    #[max_length = 32]
    name: String,
) -> Result<(), Error> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let name = name.trim();
    let (result, note) = ctx
        .data()
        .edit_store(|store| store.add_menu_item(guild_id.get(), name));
    let text = match result {
        Ok(count) => format!("已把「{name}」加進這個伺服器的菜單，現在共 {count} 道。"),
        Err(EditError::Duplicate) => format!("「{name}」已經在菜單裡了。"),
        Err(EditError::Full) => format!("菜單滿了，最多 {MAX_MENU_ITEMS} 道，先移除一些吧。"),
        Err(EditError::Invalid | EditError::Missing) => {
            format!("名稱只能用文字和數字，最多 {MAX_FOOD_CHARS} 個字，也不能是「什麼」這類的詞。")
        }
    };
    reply_privately(ctx, &format!("{text}{note}")).await
}

/// Remove a food from this server's menu
#[poise::command(
    slash_command,
    guild_only,
    rename = "remove",
    name_localized("zh-TW", "移除"),
    description_localized("zh-TW", "從這個伺服器的菜單移除一道")
)]
pub async fn menu_remove(
    ctx: Context<'_>,
    #[description = "Food or restaurant name"]
    #[name_localized("zh-TW", "名稱")]
    #[description_localized("zh-TW", "要移除的名稱")]
    #[max_length = 32]
    #[autocomplete = "autocomplete_menu"]
    name: String,
) -> Result<(), Error> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let (result, note) = ctx
        .data()
        .edit_store(|store| store.remove_menu_item(guild_id.get(), &name));
    let text = match result {
        Ok(count) => format!("已移除「{}」，菜單剩 {count} 道。", name.trim()),
        Err(_) => "菜單裡沒有這一道。".to_owned(),
    };
    reply_privately(ctx, &format!("{text}{note}")).await
}

/// Show this server's menu
#[poise::command(
    slash_command,
    guild_only,
    rename = "list",
    name_localized("zh-TW", "列表"),
    description_localized("zh-TW", "看這個伺服器的菜單")
)]
pub async fn menu_list(ctx: Context<'_>) -> Result<(), Error> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let settings = ctx.data().read_store(|store| store.guild(guild_id.get()));
    let text = if settings.menu.is_empty() {
        format!(
            "這個伺服器還沒有自訂菜單，用 /菜單 新增 加幾家常吃的店吧。\n推薦來源：{}",
            mode_label(settings.mode)
        )
    } else {
        format!(
            "這個伺服器的菜單（共 {} 道）：\n{}\n推薦來源：{}",
            settings.menu.len(),
            settings.menu.join("、"),
            mode_label(settings.mode)
        )
    };
    reply_privately(ctx, &text).await
}

/// Choose where recommendations come from
#[poise::command(
    slash_command,
    guild_only,
    rename = "mode",
    name_localized("zh-TW", "模式"),
    description_localized("zh-TW", "設定推薦要用自訂菜單、內建菜單，還是混合")
)]
pub async fn menu_mode(
    ctx: Context<'_>,
    #[description = "Where recommendations come from"]
    #[name_localized("zh-TW", "來源")]
    #[description_localized("zh-TW", "推薦從哪裡抽")]
    mode: MenuModeChoice,
) -> Result<(), Error> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let mode = MenuMode::from(mode);
    let (_, note) = ctx.data().edit_store(|store| {
        store.set_mode(guild_id.get(), mode);
        Ok::<(), EditError>(())
    });
    reply_privately(ctx, &format!("推薦來源改成：{}。{note}", mode_label(mode))).await
}

async fn autocomplete_menu(
    ctx: Context<'_>,
    partial: &str,
) -> serenity::CreateAutocompleteResponse {
    let items = match ctx.guild_id() {
        Some(guild_id) => ctx
            .data()
            .read_store(|store| store.guild(guild_id.get()).menu),
        None => Vec::new(),
    };
    autocomplete(items, partial)
}

/// Foods you don't eat; recommendations will skip them
#[poise::command(
    slash_command,
    guild_only,
    name_localized("zh-TW", "忌口"),
    description_localized("zh-TW", "設定不吃的東西，推薦時會自動避開"),
    subcommands("avoid_add", "avoid_remove", "avoid_list", "avoid_clear"),
    subcommand_required
)]
pub async fn avoid(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Add something you don't eat, such as 牛 or 辣
#[poise::command(
    slash_command,
    guild_only,
    rename = "add",
    name_localized("zh-TW", "新增"),
    description_localized("zh-TW", "新增不吃的關鍵字，例如：牛、辣、海鮮")
)]
pub async fn avoid_add(
    ctx: Context<'_>,
    #[description = "Keyword, such as 牛 or 辣"]
    #[name_localized("zh-TW", "關鍵字")]
    #[description_localized("zh-TW", "菜名裡有這個字就不推薦")]
    #[max_length = 16]
    keyword: String,
) -> Result<(), Error> {
    let user_id = ctx.author().id.get();
    let keyword = keyword.trim();
    let (result, note) = ctx
        .data()
        .edit_store(|store| store.add_avoid(user_id, keyword));
    let text = match result {
        Ok(count) => format!("記住了，之後推薦會避開有「{keyword}」的菜（共 {count} 個忌口）。"),
        Err(EditError::Duplicate) => format!("「{keyword}」已經在你的忌口裡了。"),
        Err(EditError::Full) => format!("忌口最多 {MAX_AVOID_KEYWORDS} 個，先移除一些吧。"),
        Err(EditError::Invalid | EditError::Missing) => {
            format!("關鍵字只能用文字和數字，最多 {MAX_KEYWORD_CHARS} 個字。")
        }
    };
    reply_privately(ctx, &format!("{text}{note}")).await
}

/// Remove something from your avoid list
#[poise::command(
    slash_command,
    guild_only,
    rename = "remove",
    name_localized("zh-TW", "移除"),
    description_localized("zh-TW", "移除一個忌口")
)]
pub async fn avoid_remove(
    ctx: Context<'_>,
    #[description = "Keyword to remove"]
    #[name_localized("zh-TW", "關鍵字")]
    #[description_localized("zh-TW", "要移除的關鍵字")]
    #[max_length = 16]
    #[autocomplete = "autocomplete_avoid"]
    keyword: String,
) -> Result<(), Error> {
    let user_id = ctx.author().id.get();
    let (result, note) = ctx
        .data()
        .edit_store(|store| store.remove_avoid(user_id, &keyword));
    let text = match result {
        Ok(_) => format!("已移除「{}」。", keyword.trim()),
        Err(_) => "你的忌口裡沒有這個。".to_owned(),
    };
    reply_privately(ctx, &format!("{text}{note}")).await
}

/// Show your avoid list
#[poise::command(
    slash_command,
    guild_only,
    rename = "list",
    name_localized("zh-TW", "列表"),
    description_localized("zh-TW", "看你設定的忌口")
)]
pub async fn avoid_list(ctx: Context<'_>) -> Result<(), Error> {
    let avoid = ctx
        .data()
        .read_store(|store| store.avoid_list(ctx.author().id.get()));
    let text = if avoid.is_empty() {
        "你還沒有設定忌口，用 /忌口 新增 加一個。".to_owned()
    } else {
        format!("你的忌口：{}", avoid.join("、"))
    };
    reply_privately(ctx, &text).await
}

/// Clear your avoid list
#[poise::command(
    slash_command,
    guild_only,
    rename = "clear",
    name_localized("zh-TW", "清除"),
    description_localized("zh-TW", "清掉所有忌口")
)]
pub async fn avoid_clear(ctx: Context<'_>) -> Result<(), Error> {
    let user_id = ctx.author().id.get();
    let (cleared, note) = ctx.data().edit_store(|store| {
        if store.clear_user(user_id) {
            Ok(())
        } else {
            Err(EditError::Missing)
        }
    });
    let text = match cleared {
        Ok(()) => "忌口都清掉了。",
        Err(_) => "你本來就沒有設定忌口。",
    };
    reply_privately(ctx, &format!("{text}{note}")).await
}

async fn autocomplete_avoid(
    ctx: Context<'_>,
    partial: &str,
) -> serenity::CreateAutocompleteResponse {
    let avoid = ctx
        .data()
        .read_store(|store| store.avoid_list(ctx.author().id.get()));
    autocomplete(avoid, partial)
}

/// How to use the dinner 8-ball
#[poise::command(
    slash_command,
    name_localized("zh-TW", "說明"),
    description_localized("zh-TW", "看 8-Ball 的用法")
)]
pub async fn help(ctx: Context<'_>) -> Result<(), Error> {
    reply_privately(ctx, HELP).await
}

const HELP: &str = "直接在聊天裡問就可以：
- 推薦：「晚餐吃什麼」「宵夜要吃啥」
- 判斷：「要不要吃火鍋」「今天吃拉麵嗎」（同一天問同一道，答案不變）
- 二選一：「火鍋還是拉麵」「晚餐吃壽司還是咖哩」
推薦下面有「再抽一次」，按 ❌ 可以收回回覆。

斜線指令：
- /吃什麼、/幫我選：同上，用指令問
- /揪團：抽幾個候選讓大家按按鈕投票
- /菜單：加入你們常吃的店，推薦會抽到它們
- /忌口：設定不吃的東西，推薦時自動避開";

fn autocomplete(items: Vec<String>, partial: &str) -> serenity::CreateAutocompleteResponse {
    let choices = items
        .into_iter()
        .filter(|item| item.contains(partial.trim()))
        .take(25)
        .map(|item| serenity::AutocompleteChoice::new(item.clone(), item))
        .collect();
    serenity::CreateAutocompleteResponse::new().set_choices(choices)
}

fn asking(ctx: Context<'_>) -> Asking {
    Asking {
        user_id: ctx.author().id.get(),
        channel_id: ctx.channel_id().get(),
        guild_id: ctx.guild_id().map(serenity::GuildId::get),
    }
}

fn cooldown_message(remaining: std::time::Duration) -> String {
    let seconds = remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0);
    format!("慢一點，{seconds} 秒後再問我。")
}

async fn send_answer(ctx: Context<'_>, intent: &Intent) -> Result<(), Error> {
    let state = ctx.data();
    if let Err(remaining) = state.check_cooldown(ctx.author().id.get()) {
        return reply_privately(ctx, &cooldown_message(remaining)).await;
    }

    let asking = asking(ctx);
    let answer = state.answer(intent, asking);
    let mut reply = CreateReply::default()
        .content(answer.reply.text)
        .allowed_mentions(serenity::CreateAllowedMentions::new());
    if answer.reply.recommended.is_some() {
        reply = reply.components(buttons::reroll_row(asking.user_id, answer.meal, 0));
    }
    let handle = ctx.send(reply).await?;

    match handle.message().await {
        Ok(message) => {
            state.remember_reply(ReplyRecord {
                reply_id: message.id.get(),
                trigger_id: None,
                owner_id: asking.user_id,
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
            error!(command = %ctx.command().qualified_name, %error, "command failed");
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn check_name(name: &str, path: &str) {
        let length = name.chars().count();
        assert!(
            (1..=32).contains(&length),
            "{path}: name {name:?} length {length}"
        );
        assert!(
            name.chars()
                .all(|c| (c.is_alphanumeric() || "-_'".contains(c)) && !c.is_uppercase()),
            "{path}: invalid command name {name:?}"
        );
    }

    fn check_description(description: &str, path: &str) {
        let length = description.chars().count();
        assert!(
            (1..=100).contains(&length),
            "{path}: description {description:?} length {length}"
        );
    }

    fn check_localizations(value: &Value, key: &str, path: &str, check: fn(&str, &str)) {
        if let Some(map) = value.get(key).and_then(Value::as_object) {
            for (locale, text) in map {
                check(text.as_str().unwrap(), &format!("{path}[{locale}]"));
            }
        }
    }

    fn check_command(value: &Value, path: &str) {
        let name = value["name"].as_str().unwrap();
        let path = format!("{path}/{name}");
        check_name(name, &path);
        check_localizations(value, "name_localizations", &path, check_name);
        check_description(value["description"].as_str().unwrap(), &path);
        check_localizations(value, "description_localizations", &path, check_description);

        let options = value.get("options").and_then(Value::as_array);
        let options = options.map(Vec::as_slice).unwrap_or_default();
        assert!(options.len() <= 25, "{path}: too many options");
        let mut seen_optional = false;
        for option in options {
            let is_subcommand = matches!(option["type"].as_u64(), Some(1 | 2));
            if !is_subcommand {
                let required = option
                    .get("required")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                assert!(
                    !(required && seen_optional),
                    "{path}: required option after optional"
                );
                seen_optional |= !required;
            }
            for choice in option
                .get("choices")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                check_description(choice["name"].as_str().unwrap(), &format!("{path} choice"));
                check_localizations(choice, "name_localizations", &path, check_description);
            }
            check_command(option, &path);
        }
    }

    #[test]
    fn slash_commands_are_valid_for_discord() {
        let commands = all();
        let payload = poise::builtins::create_application_commands(&commands);
        let json = serde_json::to_value(&payload).unwrap();
        let commands = json.as_array().unwrap();
        assert_eq!(commands.len(), 6);
        for command in commands {
            check_command(command, "");
        }
    }

    #[test]
    fn help_fits_in_one_message() {
        assert!(HELP.chars().count() < 2_000);
    }
}
