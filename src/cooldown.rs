//! Per-user cooldown so one person cannot make the bot flood a channel.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Entries kept before expired ones are pruned.
const PRUNE_THRESHOLD: usize = 4_096;

#[derive(Debug)]
pub struct Cooldown {
    window: Duration,
    last_seen: HashMap<u64, Instant>,
}

impl Cooldown {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            last_seen: HashMap::new(),
        }
    }

    /// Records a request from `key` at `now`. Returns `Err(remaining)` while
    /// the previous accepted request is still inside the window.
    pub fn check(&mut self, key: u64, now: Instant) -> Result<(), Duration> {
        if self.window.is_zero() {
            return Ok(());
        }
        if let Some(last) = self.last_seen.get(&key) {
            let elapsed = now.saturating_duration_since(*last);
            if elapsed < self.window {
                return Err(self.window - elapsed);
            }
        }

        if self.last_seen.len() >= PRUNE_THRESHOLD {
            let window = self.window;
            self.last_seen
                .retain(|_, last| now.saturating_duration_since(*last) < window);
        }
        self.last_seen.insert(key, now);
        Ok(())
    }

    pub fn tracked(&self) -> usize {
        self.last_seen.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Duration = Duration::from_secs(3);

    #[test]
    fn first_request_is_allowed() {
        let mut cooldown = Cooldown::new(WINDOW);
        assert_eq!(cooldown.check(1, Instant::now()), Ok(()));
    }

    #[test]
    fn repeat_inside_window_is_rejected_with_remaining_time() {
        let mut cooldown = Cooldown::new(WINDOW);
        let start = Instant::now();
        cooldown.check(1, start).unwrap();
        assert_eq!(
            cooldown.check(1, start + Duration::from_secs(1)),
            Err(Duration::from_secs(2))
        );
    }

    #[test]
    fn rejected_requests_do_not_extend_the_window() {
        let mut cooldown = Cooldown::new(WINDOW);
        let start = Instant::now();
        cooldown.check(1, start).unwrap();
        assert!(cooldown.check(1, start + Duration::from_secs(2)).is_err());
        assert_eq!(cooldown.check(1, start + WINDOW), Ok(()));
    }

    #[test]
    fn users_are_independent() {
        let mut cooldown = Cooldown::new(WINDOW);
        let start = Instant::now();
        cooldown.check(1, start).unwrap();
        assert_eq!(cooldown.check(2, start), Ok(()));
    }

    #[test]
    fn zero_window_disables_the_cooldown() {
        let mut cooldown = Cooldown::new(Duration::ZERO);
        let now = Instant::now();
        assert_eq!(cooldown.check(1, now), Ok(()));
        assert_eq!(cooldown.check(1, now), Ok(()));
        assert_eq!(cooldown.tracked(), 0);
    }

    #[test]
    fn expired_entries_are_pruned() {
        let mut cooldown = Cooldown::new(WINDOW);
        let start = Instant::now();
        for key in 0..PRUNE_THRESHOLD as u64 {
            cooldown.check(key, start).unwrap();
        }
        cooldown.check(u64::MAX, start + WINDOW).unwrap();
        assert_eq!(cooldown.tracked(), 1);
    }
}
