//! Dinner 8-Ball: a Discord bot that answers "what should I eat?" questions.
//!
//! Everything except [`discord`] is pure logic so it can be unit tested without
//! a Discord connection.

pub mod clock;
pub mod config;
pub mod cooldown;
pub mod data;
pub mod discord;
pub mod intent;
pub mod ledger;
pub mod meal;
pub mod recent;
pub mod reply;
pub mod rng;
pub mod store;
pub mod text;
pub mod vote;
