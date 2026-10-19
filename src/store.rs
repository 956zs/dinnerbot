//! Per-server menus and per-user food preferences, saved as a JSON file.
//!
//! The file is small and changes rarely, so it is rewritten in full on every
//! change. Writes go to a temporary file, are flushed to disk, and are then
//! renamed over the old file, so neither a crash nor a power loss leaves a
//! half-written file behind (on filesystems that honour fsync).

use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::data::MenuMode;
use crate::text;

/// Most custom foods one server can keep.
pub const MAX_MENU_ITEMS: usize = 100;
/// Most avoided keywords one user can keep.
pub const MAX_AVOID_KEYWORDS: usize = 20;
/// Longest avoided keyword, in characters.
pub const MAX_KEYWORD_CHARS: usize = 8;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuildSettings {
    #[serde(default)]
    pub menu: Vec<String>,
    #[serde(default)]
    pub mode: MenuMode,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserPrefs {
    #[serde(default)]
    pub avoid: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct StoreData {
    #[serde(default)]
    guilds: BTreeMap<u64, GuildSettings>,
    #[serde(default)]
    users: BTreeMap<u64, UserPrefs>,
}

#[derive(Debug)]
pub enum StoreError {
    Read(PathBuf, io::Error),
    Parse(PathBuf, String),
    Write(PathBuf, io::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Read(path, error) => write!(f, "cannot read {}: {error}", path.display()),
            StoreError::Parse(path, error) => {
                write!(f, "{} is not a valid state file: {error}", path.display())
            }
            StoreError::Write(path, error) => {
                write!(f, "cannot save {}: {error}", path.display())
            }
        }
    }
}

impl std::error::Error for StoreError {}

/// Why an edit was refused. Shown to the user, so no internals here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditError {
    Invalid,
    Duplicate,
    Full,
    Missing,
}

#[derive(Debug)]
pub struct Store {
    path: Option<PathBuf>,
    data: StoreData,
}

impl Store {
    /// A store that is never written to disk; for tests and fallback.
    pub fn in_memory() -> Self {
        Self {
            path: None,
            data: StoreData::default(),
        }
    }

    /// Loads the state file. A missing file means an empty store; a broken
    /// file is an error, so it is never silently overwritten.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let path = path.into();
        let data = match std::fs::read_to_string(&path) {
            Ok(json) => serde_json::from_str(&json)
                .map_err(|error| StoreError::Parse(path.clone(), error.to_string()))?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => StoreData::default(),
            Err(error) => return Err(StoreError::Read(path, error)),
        };
        Ok(Self {
            path: Some(path),
            data,
        })
    }

    pub fn guild(&self, guild_id: u64) -> GuildSettings {
        self.data.guilds.get(&guild_id).cloned().unwrap_or_default()
    }

    pub fn avoid_list(&self, user_id: u64) -> Vec<String> {
        self.data
            .users
            .get(&user_id)
            .map(|prefs| prefs.avoid.clone())
            .unwrap_or_default()
    }

    /// Adds a food to a server's menu and returns the new menu size.
    pub fn add_menu_item(&mut self, guild_id: u64, item: &str) -> Result<usize, EditError> {
        let item = item.trim();
        if !is_valid_menu_item(item) {
            return Err(EditError::Invalid);
        }
        let menu = &mut self.data.guilds.entry(guild_id).or_default().menu;
        if menu
            .iter()
            .any(|existing| text::fold(existing) == text::fold(item))
        {
            return Err(EditError::Duplicate);
        }
        if menu.len() >= MAX_MENU_ITEMS {
            return Err(EditError::Full);
        }
        menu.push(item.to_owned());
        Ok(menu.len())
    }

    /// Removes a food from a server's menu and returns the new menu size.
    pub fn remove_menu_item(&mut self, guild_id: u64, item: &str) -> Result<usize, EditError> {
        let settings = self
            .data
            .guilds
            .get_mut(&guild_id)
            .ok_or(EditError::Missing)?;
        let index = settings
            .menu
            .iter()
            .position(|existing| existing == item.trim())
            .ok_or(EditError::Missing)?;
        settings.menu.remove(index);
        let remaining = settings.menu.len();
        self.prune_guild(guild_id);
        Ok(remaining)
    }

    pub fn set_mode(&mut self, guild_id: u64, mode: MenuMode) {
        self.data.guilds.entry(guild_id).or_default().mode = mode;
        self.prune_guild(guild_id);
    }

    /// Adds an avoided keyword and returns how many the user has.
    pub fn add_avoid(&mut self, user_id: u64, keyword: &str) -> Result<usize, EditError> {
        let keyword = keyword.trim();
        if !is_valid_keyword(keyword) {
            return Err(EditError::Invalid);
        }
        let avoid = &mut self.data.users.entry(user_id).or_default().avoid;
        if avoid
            .iter()
            .any(|existing| text::fold(existing) == text::fold(keyword))
        {
            return Err(EditError::Duplicate);
        }
        if avoid.len() >= MAX_AVOID_KEYWORDS {
            return Err(EditError::Full);
        }
        avoid.push(keyword.to_owned());
        Ok(avoid.len())
    }

    /// Removes an avoided keyword and returns how many remain.
    pub fn remove_avoid(&mut self, user_id: u64, keyword: &str) -> Result<usize, EditError> {
        let prefs = self
            .data
            .users
            .get_mut(&user_id)
            .ok_or(EditError::Missing)?;
        let index = prefs
            .avoid
            .iter()
            .position(|existing| existing == keyword.trim())
            .ok_or(EditError::Missing)?;
        prefs.avoid.remove(index);
        let remaining = prefs.avoid.len();
        if remaining == 0 {
            self.data.users.remove(&user_id);
        }
        Ok(remaining)
    }

    /// Forgets all of a user's preferences. Returns whether there were any.
    pub fn clear_user(&mut self, user_id: u64) -> bool {
        self.data.users.remove(&user_id).is_some()
    }

    /// Writes the store to its file, if it has one.
    pub fn save(&self) -> Result<(), StoreError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let json = serde_json::to_string_pretty(&self.data)
            .map_err(|error| StoreError::Write(path.clone(), io::Error::other(error)))?;
        write_atomically(path, json.as_bytes())
            .map_err(|error| StoreError::Write(path.clone(), error))
    }

    /// Drops a server entry that holds nothing but defaults.
    fn prune_guild(&mut self, guild_id: u64) {
        if self.data.guilds.get(&guild_id) == Some(&GuildSettings::default()) {
            self.data.guilds.remove(&guild_id);
        }
    }
}

fn write_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(directory)?;

    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    {
        let mut file = File::create(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
    }
    std::fs::rename(&temporary, path)?;

    // Make the rename itself durable. Some platforms cannot open a
    // directory; the data is already on disk then, only the rename is not.
    if let Ok(directory) = File::open(directory) {
        let _ = directory.sync_all();
    }
    Ok(())
}

/// A server's own food: safe characters, not a question word.
pub fn is_valid_menu_item(item: &str) -> bool {
    text::is_safe_food_text(item, text::MAX_FOOD_CHARS) && !text::is_generic(item)
}

/// An avoided keyword such as "牛" or "辣".
pub fn is_valid_keyword(keyword: &str) -> bool {
    text::is_safe_food_text(keyword, MAX_KEYWORD_CHARS)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUILD: u64 = 1;
    const USER: u64 = 2;

    fn temp_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("dinnerbot-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path.push("state.json");
        path
    }

    #[test]
    fn empty_store_has_defaults() {
        let store = Store::in_memory();
        assert_eq!(store.guild(GUILD), GuildSettings::default());
        assert!(store.avoid_list(USER).is_empty());
    }

    #[test]
    fn menu_items_are_added_and_removed() {
        let mut store = Store::in_memory();
        assert_eq!(store.add_menu_item(GUILD, " 巷口牛肉麵 "), Ok(1));
        assert_eq!(store.add_menu_item(GUILD, "阿姨便當"), Ok(2));
        assert_eq!(store.guild(GUILD).menu, vec!["巷口牛肉麵", "阿姨便當"]);
        assert_eq!(store.remove_menu_item(GUILD, "巷口牛肉麵"), Ok(1));
        assert_eq!(
            store.remove_menu_item(GUILD, "巷口牛肉麵"),
            Err(EditError::Missing)
        );
        assert_eq!(
            store.remove_menu_item(99, "阿姨便當"),
            Err(EditError::Missing)
        );
    }

    #[test]
    fn menu_rejects_bad_duplicate_and_overflow() {
        let mut store = Store::in_memory();
        for item in [
            "",
            "@everyone",
            "什麼",
            "<@1>",
            &"肉".repeat(text::MAX_FOOD_CHARS + 1),
        ] {
            assert_eq!(
                store.add_menu_item(GUILD, item),
                Err(EditError::Invalid),
                "{item}"
            );
        }
        store.add_menu_item(GUILD, "IKEA").unwrap();
        assert_eq!(
            store.add_menu_item(GUILD, "ikea"),
            Err(EditError::Duplicate)
        );
        store.remove_menu_item(GUILD, "IKEA").unwrap();
        store.add_menu_item(GUILD, "火鍋").unwrap();
        assert_eq!(
            store.add_menu_item(GUILD, "火鍋"),
            Err(EditError::Duplicate)
        );
        for n in 1..MAX_MENU_ITEMS {
            store.add_menu_item(GUILD, &format!("菜{n}")).unwrap();
        }
        assert_eq!(store.add_menu_item(GUILD, "再一道"), Err(EditError::Full));
    }

    #[test]
    fn menu_accepts_names_starting_with_demonstratives() {
        let mut store = Store::in_memory();
        assert_eq!(store.add_menu_item(GUILD, "那家牛肉麵"), Ok(1));
    }

    #[test]
    fn avoid_keywords_are_managed() {
        let mut store = Store::in_memory();
        assert_eq!(store.add_avoid(USER, "牛"), Ok(1));
        assert_eq!(store.add_avoid(USER, "辣"), Ok(2));
        assert_eq!(store.add_avoid(USER, "牛"), Err(EditError::Duplicate));
        assert_eq!(store.add_avoid(USER, "ikea"), Ok(3));
        assert_eq!(store.add_avoid(USER, "ＩＫＥＡ"), Err(EditError::Duplicate));
        store.remove_avoid(USER, "ikea").unwrap();
        assert_eq!(store.add_avoid(USER, "@here"), Err(EditError::Invalid));
        assert_eq!(
            store.add_avoid(USER, &"辣".repeat(MAX_KEYWORD_CHARS + 1)),
            Err(EditError::Invalid)
        );
        assert_eq!(store.avoid_list(USER), vec!["牛", "辣"]);
        assert_eq!(store.remove_avoid(USER, "牛"), Ok(1));
        assert_eq!(store.remove_avoid(USER, "牛"), Err(EditError::Missing));
        assert!(store.clear_user(USER));
        assert!(!store.clear_user(USER));
    }

    #[test]
    fn avoid_list_is_capped() {
        let mut store = Store::in_memory();
        for n in 0..MAX_AVOID_KEYWORDS {
            store.add_avoid(USER, &format!("忌{n}")).unwrap();
        }
        assert_eq!(store.add_avoid(USER, "再一個"), Err(EditError::Full));
    }

    #[test]
    fn mode_defaults_to_mixed_and_can_change() {
        let mut store = Store::in_memory();
        assert_eq!(store.guild(GUILD).mode, MenuMode::Mixed);
        store.set_mode(GUILD, MenuMode::CustomOnly);
        assert_eq!(store.guild(GUILD).mode, MenuMode::CustomOnly);
    }

    #[test]
    fn save_and_reload_round_trips() {
        let path = temp_path("roundtrip");
        let mut store = Store::open(&path).unwrap();
        store.add_menu_item(GUILD, "巷口牛肉麵").unwrap();
        store.set_mode(GUILD, MenuMode::CustomOnly);
        store.add_avoid(USER, "辣").unwrap();
        store.save().unwrap();

        let reloaded = Store::open(&path).unwrap();
        assert_eq!(reloaded.guild(GUILD).menu, vec!["巷口牛肉麵"]);
        assert_eq!(reloaded.guild(GUILD).mode, MenuMode::CustomOnly);
        assert_eq!(reloaded.avoid_list(USER), vec!["辣"]);
        assert!(!path.with_extension("json.tmp").exists());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn missing_file_is_empty_but_broken_file_is_an_error() {
        let path = temp_path("broken");
        assert!(Store::open(&path).unwrap().avoid_list(USER).is_empty());

        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json").unwrap();
        assert!(matches!(Store::open(&path), Err(StoreError::Parse(..))));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn in_memory_save_is_a_no_op() {
        assert!(Store::in_memory().save().is_ok());
    }
}
