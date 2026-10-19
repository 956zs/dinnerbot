//! Runtime settings read from environment variables (and `.env`).

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

const DEFAULT_COOLDOWN_SECS: u64 = 3;
const MAX_COOLDOWN_SECS: u64 = 3_600;
const DEFAULT_UTC_OFFSET_HOURS: i32 = 8;
const DEFAULT_STATE_PATH: &str = "dinnerbot-state.json";

#[derive(Clone)]
pub struct Config {
    pub token: String,
    /// Minimum time between two answers for the same user.
    pub cooldown: Duration,
    /// Offset used to tell breakfast from late night and to roll the daily fate.
    pub utc_offset_seconds: i32,
    /// Optional menu file that replaces the built-in one.
    pub data_path: Option<PathBuf>,
    /// Where server menus and user preferences are saved.
    pub state_path: PathBuf,
    /// Register slash commands in this guild only (instant, for testing).
    pub dev_guild_id: Option<u64>,
}

// Hand-written so the token never ends up in logs.
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("token", &"<redacted>")
            .field("cooldown", &self.cooldown)
            .field("utc_offset_seconds", &self.utc_offset_seconds)
            .field("data_path", &self.data_path)
            .field("state_path", &self.state_path)
            .field("dev_guild_id", &self.dev_guild_id)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    MissingToken,
    Invalid {
        name: &'static str,
        expected: &'static str,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::MissingToken => {
                write!(
                    f,
                    "missing DISCORD_TOKEN: copy .env.example to .env and set your bot token"
                )
            }
            ConfigError::Invalid { name, expected } => write!(f, "{name} must be {expected}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Builds the config from any variable source; used by tests.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let read = |name: &str| {
            lookup(name)
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        };

        let token = read("DISCORD_TOKEN").ok_or(ConfigError::MissingToken)?;

        let cooldown_secs = match read("COOLDOWN_SECS") {
            None => DEFAULT_COOLDOWN_SECS,
            Some(value) => value
                .parse::<u64>()
                .ok()
                .filter(|secs| *secs <= MAX_COOLDOWN_SECS)
                .ok_or(ConfigError::Invalid {
                    name: "COOLDOWN_SECS",
                    expected: "a whole number of seconds from 0 to 3600",
                })?,
        };

        let utc_offset_hours = match read("UTC_OFFSET_HOURS") {
            None => DEFAULT_UTC_OFFSET_HOURS,
            Some(value) => value
                .parse::<i32>()
                .ok()
                .filter(|hours| (-12..=14).contains(hours))
                .ok_or(ConfigError::Invalid {
                    name: "UTC_OFFSET_HOURS",
                    expected: "a whole number of hours from -12 to 14",
                })?,
        };

        let dev_guild_id = match read("DEV_GUILD_ID") {
            None => None,
            Some(value) => Some(value.parse::<u64>().ok().filter(|id| *id != 0).ok_or(
                ConfigError::Invalid {
                    name: "DEV_GUILD_ID",
                    expected: "a Discord server ID",
                },
            )?),
        };

        Ok(Self {
            token,
            cooldown: Duration::from_secs(cooldown_secs),
            utc_offset_seconds: utc_offset_hours * 3_600,
            data_path: read("DINNERBOT_DATA").map(PathBuf::from),
            state_path: read("DINNERBOT_STATE")
                .map_or_else(|| PathBuf::from(DEFAULT_STATE_PATH), PathBuf::from),
            dev_guild_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn config(vars: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        Config::from_lookup(|name| vars.get(name).cloned())
    }

    #[test]
    fn defaults_apply_when_only_the_token_is_set() {
        let config = config(&[("DISCORD_TOKEN", "abc")]).unwrap();
        assert_eq!(config.token, "abc");
        assert_eq!(config.cooldown, Duration::from_secs(3));
        assert_eq!(config.utc_offset_seconds, 8 * 3_600);
        assert_eq!(config.data_path, None);
        assert_eq!(config.state_path, PathBuf::from("dinnerbot-state.json"));
        assert_eq!(config.dev_guild_id, None);
    }

    #[test]
    fn missing_or_blank_token_is_an_error() {
        assert_eq!(config(&[]).unwrap_err(), ConfigError::MissingToken);
        assert_eq!(
            config(&[("DISCORD_TOKEN", "  ")]).unwrap_err(),
            ConfigError::MissingToken
        );
    }

    #[test]
    fn reads_every_option() {
        let config = config(&[
            ("DISCORD_TOKEN", "abc"),
            ("COOLDOWN_SECS", "0"),
            ("UTC_OFFSET_HOURS", "-5"),
            ("DINNERBOT_DATA", "/tmp/menu.json"),
            ("DEV_GUILD_ID", "123"),
            ("DINNERBOT_STATE", "/var/lib/dinnerbot/state.json"),
        ])
        .unwrap();
        assert_eq!(config.cooldown, Duration::ZERO);
        assert_eq!(config.utc_offset_seconds, -5 * 3_600);
        assert_eq!(config.data_path, Some(PathBuf::from("/tmp/menu.json")));
        assert_eq!(config.dev_guild_id, Some(123));
        assert_eq!(
            config.state_path,
            PathBuf::from("/var/lib/dinnerbot/state.json")
        );
    }

    #[test]
    fn rejects_out_of_range_values() {
        for (name, value) in [
            ("COOLDOWN_SECS", "-1"),
            ("COOLDOWN_SECS", "3601"),
            ("COOLDOWN_SECS", "soon"),
            ("UTC_OFFSET_HOURS", "15"),
            ("UTC_OFFSET_HOURS", "-13"),
            ("DEV_GUILD_ID", "0"),
            ("DEV_GUILD_ID", "abc"),
        ] {
            let result = config(&[("DISCORD_TOKEN", "abc"), (name, value)]);
            assert!(
                matches!(result, Err(ConfigError::Invalid { name: n, .. }) if n == name),
                "{name}={value}"
            );
        }
    }

    #[test]
    fn debug_output_hides_the_token() {
        let config = config(&[("DISCORD_TOKEN", "super-secret")]).unwrap();
        assert!(!format!("{config:?}").contains("super-secret"));
    }
}
