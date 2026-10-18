//! Remembers the last few foods suggested in each channel to avoid repeats.

use std::collections::{HashMap, VecDeque};

#[derive(Debug)]
pub struct RecentPicks {
    per_channel: usize,
    max_channels: usize,
    channels: HashMap<u64, VecDeque<String>>,
}

impl RecentPicks {
    pub fn new(per_channel: usize, max_channels: usize) -> Self {
        Self {
            per_channel,
            max_channels: max_channels.max(1),
            channels: HashMap::new(),
        }
    }

    pub fn recent(&self, channel_id: u64) -> Vec<String> {
        self.channels
            .get(&channel_id)
            .map(|foods| foods.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn record(&mut self, channel_id: u64, food: String) {
        if self.per_channel == 0 {
            return;
        }
        if !self.channels.contains_key(&channel_id) && self.channels.len() >= self.max_channels {
            // Rarely hit; forgetting everything is cheaper than tracking age.
            self.channels.clear();
        }
        let foods = self.channels.entry(channel_id).or_default();
        if foods.len() == self.per_channel {
            foods.pop_front();
        }
        foods.push_back(food);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_latest_foods_per_channel() {
        let mut recent = RecentPicks::new(2, 10);
        recent.record(1, "粥".to_owned());
        recent.record(1, "麵".to_owned());
        recent.record(1, "飯".to_owned());
        recent.record(2, "湯".to_owned());
        assert_eq!(recent.recent(1), vec!["麵".to_owned(), "飯".to_owned()]);
        assert_eq!(recent.recent(2), vec!["湯".to_owned()]);
        assert!(recent.recent(3).is_empty());
    }

    #[test]
    fn zero_history_records_nothing() {
        let mut recent = RecentPicks::new(0, 10);
        recent.record(1, "粥".to_owned());
        assert!(recent.recent(1).is_empty());
    }

    #[test]
    fn channel_limit_resets_the_map() {
        let mut recent = RecentPicks::new(2, 2);
        recent.record(1, "粥".to_owned());
        recent.record(2, "麵".to_owned());
        recent.record(2, "飯".to_owned());
        recent.record(3, "湯".to_owned());
        assert!(recent.recent(1).is_empty());
        assert_eq!(recent.recent(3), vec!["湯".to_owned()]);
    }
}
