//! Discord wiring: shared state, startup and shutdown.

mod buttons;
mod commands;
mod events;
mod votes;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use poise::serenity_prelude as serenity;
use tracing::{error, info};

use crate::clock::LocalTime;
use crate::config::Config;
use crate::cooldown::Cooldown;
use crate::data::{FoodData, PickRules};
use crate::intent::Intent;
use crate::ledger::{ReplyLedger, ReplyRecord};
use crate::meal::Meal;
use crate::recent::RecentPicks;
use crate::reply::{self, Asker, Reply, Situation};
use crate::rng::SplitMix64;
use crate::store::Store;
use crate::vote::Vote;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, BotState, Error>;

/// Reaction that lets the asker (or a moderator) dismiss a reply.
pub const DISMISS_EMOJI: &str = "❌";

const LEDGER_CAPACITY: usize = 2_048;
const RECENT_PER_CHANNEL: usize = 5;
const RECENT_MAX_CHANNELS: usize = 4_096;

/// A vote that is still open. Votes are keyed by channel: one per channel.
pub(crate) struct ActiveVote {
    vote: Vote,
    /// `None` while the vote message is still being posted.
    message_id: Option<u64>,
    /// Held while a press or the close is talking to Discord, so updates
    /// reach the message in order and a late press cannot hide the result.
    gate: Arc<tokio::sync::Mutex<()>>,
}

type Votes = Arc<Mutex<HashMap<u64, ActiveVote>>>;

/// Appended to replies when a change could not be written to disk.
const SAVE_FAILED: &str = "\n（不過存檔失敗，Bot 重開後這個變更會不見，請通知管理員。）";

pub struct BotState {
    data: FoodData,
    utc_offset_seconds: i32,
    cooldown: Mutex<Cooldown>,
    ledger: Mutex<ReplyLedger>,
    recent: Mutex<RecentPicks>,
    store: Mutex<Store>,
    votes: Votes,
}

/// Who is asking, and where.
#[derive(Debug, Clone, Copy)]
struct Asking {
    user_id: u64,
    channel_id: u64,
    guild_id: Option<u64>,
}

/// A reply and the meal it was about.
struct Answer {
    reply: Reply,
    meal: Meal,
}

impl BotState {
    pub fn new(data: FoodData, store: Store, config: &Config) -> Self {
        Self {
            data,
            utc_offset_seconds: config.utc_offset_seconds,
            cooldown: Mutex::new(Cooldown::new(config.cooldown)),
            ledger: Mutex::new(ReplyLedger::new(LEDGER_CAPACITY)),
            recent: Mutex::new(RecentPicks::new(RECENT_PER_CHANNEL, RECENT_MAX_CHANNELS)),
            store: Mutex::new(store),
            votes: Arc::default(),
        }
    }

    fn check_cooldown(&self, user_id: u64) -> Result<(), Duration> {
        lock(&self.cooldown).check(user_id, Instant::now())
    }

    fn now(&self) -> LocalTime {
        LocalTime::now(self.utc_offset_seconds)
    }

    fn current_meal(&self) -> Meal {
        Meal::from_hour(self.now().hour)
    }

    /// Answers a question using the server's menu and the asker's avoids.
    fn answer(&self, intent: &Intent, asking: Asking) -> Answer {
        let now = self.now();
        let meal = intent.meal().unwrap_or(Meal::from_hour(now.hour));
        let recent = lock(&self.recent).recent(asking.channel_id);
        let (settings, avoid) = {
            let store = lock(&self.store);
            let settings = asking
                .guild_id
                .map(|guild_id| store.guild(guild_id))
                .unwrap_or_default();
            (settings, store.avoid_list(asking.user_id))
        };

        let situation = Situation {
            asker: Asker {
                user_id: asking.user_id,
                day: now.day,
            },
            default_meal: meal,
            recent: &recent,
            custom: &settings.menu,
            mode: settings.mode,
            avoid: &avoid,
        };
        let reply = reply::respond(
            intent,
            &self.data,
            &situation,
            &mut SplitMix64::from_entropy(),
        );
        if let Some(food) = &reply.recommended {
            lock(&self.recent).record(asking.channel_id, food.clone());
        }
        Answer { reply, meal }
    }

    /// Up to `count` different foods for a vote.
    fn vote_candidates(&self, count: usize, meal: Meal, asking: Asking) -> Vec<String> {
        let (settings, avoid) = {
            let store = lock(&self.store);
            let settings = asking
                .guild_id
                .map(|guild_id| store.guild(guild_id))
                .unwrap_or_default();
            (settings, store.avoid_list(asking.user_id))
        };

        let mut rng = SplitMix64::from_entropy();
        let mut chosen: Vec<String> = Vec::new();
        for _ in 0..count * 10 {
            if chosen.len() == count {
                break;
            }
            let rules = PickRules {
                meal,
                recent: &chosen,
                custom: &settings.menu,
                mode: settings.mode,
                avoid: &avoid,
            };
            match self.data.pick_food(&rules, &mut rng) {
                Some(food) if !chosen.iter().any(|existing| existing == food) => {
                    chosen.push(food.to_owned());
                }
                Some(_) => {}
                None => break,
            }
        }
        chosen
    }

    fn read_store<T>(&self, read: impl FnOnce(&Store) -> T) -> T {
        read(&lock(&self.store))
    }

    /// Applies an edit and saves the store when the edit succeeded. The
    /// returned note is empty, or tells the user the save failed.
    fn edit_store<T, E>(
        &self,
        edit: impl FnOnce(&mut Store) -> Result<T, E>,
    ) -> (Result<T, E>, &'static str) {
        let mut store = lock(&self.store);
        let result = edit(&mut store);
        let mut note = "";
        if result.is_ok() {
            if let Err(save_error) = store.save() {
                error!(error = %save_error, "could not save state; the change lasts until restart");
                note = SAVE_FAILED;
            }
        }
        (result, note)
    }

    /// Returns `false` when the question was deleted before the reply was
    /// stored; the caller should then delete the reply.
    fn remember_reply(&self, record: ReplyRecord) -> bool {
        lock(&self.ledger).insert(record)
    }

    fn reply_owner(&self, reply_id: u64) -> Option<u64> {
        lock(&self.ledger).owner_of(reply_id)
    }

    fn forget_reply(&self, reply_id: u64) {
        lock(&self.ledger).remove_reply(reply_id);
    }

    /// See [`ReplyLedger::message_deleted`].
    fn message_deleted(&self, message_id: u64) -> Option<ReplyRecord> {
        lock(&self.votes).retain(|_, active| active.message_id != Some(message_id));
        lock(&self.ledger).message_deleted(message_id)
    }
}

/// A poisoned lock only means another task panicked mid-update; the data
/// inside (caches and counters) is still usable.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn dismiss_reaction() -> serenity::ReactionType {
    serenity::ReactionType::Unicode(DISMISS_EMOJI.to_owned())
}

/// Server admins and anyone who can manage messages in the channel count as
/// moderators. Threads take their permissions from the parent channel.
async fn is_moderator(
    ctx: &serenity::Context,
    guild_id: Option<serenity::GuildId>,
    channel_id: serenity::ChannelId,
    user_id: serenity::UserId,
    member: Option<&serenity::Member>,
) -> bool {
    let Some(guild_id) = guild_id else {
        return false;
    };

    let fetched;
    let member = match member {
        Some(member) => member,
        None => match guild_id.member(ctx, user_id).await {
            Ok(member) => {
                fetched = member;
                &fetched
            }
            Err(error) => {
                tracing::warn!(%error, "could not load member to check permissions");
                return false;
            }
        },
    };

    let Some(guild) = ctx.cache.guild(guild_id) else {
        return false;
    };
    let channel = guild.channels.get(&channel_id).or_else(|| {
        guild
            .threads
            .iter()
            .find(|thread| thread.id == channel_id)
            .and_then(|thread| thread.parent_id)
            .and_then(|parent_id| guild.channels.get(&parent_id))
    });
    let permissions = match channel {
        Some(channel) => guild.user_permissions_in(channel, member),
        None => guild.member_permissions(member),
    };
    permissions.administrator() || permissions.manage_messages()
}

/// Connects to Discord and runs until the process is asked to stop.
pub async fn run(config: Config, data: FoodData, store: Store) -> Result<(), Error> {
    let intents = serenity::GatewayIntents::GUILDS
        | serenity::GatewayIntents::GUILD_MESSAGES
        | serenity::GatewayIntents::GUILD_MESSAGE_REACTIONS
        | serenity::GatewayIntents::MESSAGE_CONTENT;

    let state = BotState::new(data, store, &config);
    let dev_guild_id = config.dev_guild_id.map(serenity::GuildId::new);

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: commands::all(),
            event_handler: |framework, event| Box::pin(events::handle(framework, event)),
            on_error: |error| Box::pin(commands::on_error(error)),
            // Nothing the bot sends may ping anyone.
            allowed_mentions: Some(serenity::CreateAllowedMentions::new()),
            ..Default::default()
        })
        .setup(move |ctx, ready, framework| {
            Box::pin(async move {
                info!(user = %ready.user.name, guilds = ready.guilds.len(), "logged in");
                // Chat replies work without slash commands, so a failed
                // registration is logged instead of stalling the bot.
                let commands = &framework.options().commands;
                let registered = match dev_guild_id {
                    Some(guild_id) => {
                        poise::builtins::register_in_guild(ctx, commands, guild_id).await
                    }
                    None => poise::builtins::register_globally(ctx, commands).await,
                };
                match registered {
                    Ok(()) => info!("slash commands registered"),
                    Err(error) => error!(%error, "could not register slash commands"),
                }
                Ok(state)
            })
        })
        .build();

    let mut client = serenity::ClientBuilder::new(&config.token, intents)
        .framework(framework)
        .await?;

    let shard_manager = client.shard_manager.clone();
    tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        info!("shutting down");
        shard_manager.shutdown_all().await;
    });

    client.start().await?;
    Ok(())
}

async fn wait_for_shutdown_signal() {
    let ctrl_c = async {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}
