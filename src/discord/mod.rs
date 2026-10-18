//! Discord wiring: shared state, startup and shutdown.

mod commands;
mod events;

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use poise::serenity_prelude as serenity;
use tracing::{error, info};

use crate::clock::LocalTime;
use crate::config::Config;
use crate::cooldown::Cooldown;
use crate::data::FoodData;
use crate::intent::Intent;
use crate::ledger::{ReplyLedger, ReplyRecord};
use crate::meal::Meal;
use crate::recent::RecentPicks;
use crate::reply::{self, Asker};
use crate::rng::SplitMix64;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, BotState, Error>;

/// Reaction that lets the asker (or a moderator) dismiss a reply.
pub const DISMISS_EMOJI: &str = "❌";

const LEDGER_CAPACITY: usize = 2_048;
const RECENT_PER_CHANNEL: usize = 5;
const RECENT_MAX_CHANNELS: usize = 4_096;

pub struct BotState {
    data: FoodData,
    utc_offset_seconds: i32,
    cooldown: Mutex<Cooldown>,
    ledger: Mutex<ReplyLedger>,
    recent: Mutex<RecentPicks>,
}

impl BotState {
    pub fn new(data: FoodData, config: &Config) -> Self {
        Self {
            data,
            utc_offset_seconds: config.utc_offset_seconds,
            cooldown: Mutex::new(Cooldown::new(config.cooldown)),
            ledger: Mutex::new(ReplyLedger::new(LEDGER_CAPACITY)),
            recent: Mutex::new(RecentPicks::new(RECENT_PER_CHANNEL, RECENT_MAX_CHANNELS)),
        }
    }

    fn check_cooldown(&self, user_id: u64) -> Result<(), Duration> {
        lock(&self.cooldown).check(user_id, Instant::now())
    }

    fn answer(&self, intent: &Intent, user_id: u64, channel_id: u64) -> String {
        let now = LocalTime::now(self.utc_offset_seconds);
        let avoid = lock(&self.recent).recent(channel_id);
        let reply = reply::respond(
            intent,
            &self.data,
            Asker {
                user_id,
                day: now.day,
            },
            Meal::from_hour(now.hour),
            &avoid,
            &mut SplitMix64::from_entropy(),
        );
        if let Some(food) = reply.recommended {
            lock(&self.recent).record(channel_id, food);
        }
        reply.text
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

/// Connects to Discord and runs until the process is asked to stop.
pub async fn run(config: Config, data: FoodData) -> Result<(), Error> {
    let intents = serenity::GatewayIntents::GUILDS
        | serenity::GatewayIntents::GUILD_MESSAGES
        | serenity::GatewayIntents::GUILD_MESSAGE_REACTIONS
        | serenity::GatewayIntents::MESSAGE_CONTENT;

    let state = BotState::new(data, &config);
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
