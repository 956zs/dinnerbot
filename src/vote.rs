//! Group votes: the bot offers a few foods and everyone votes with buttons.

use std::collections::HashMap;

use crate::meal::Meal;
use crate::rng::{self, RandomSource};

pub const MIN_OPTIONS: usize = 2;
pub const MAX_OPTIONS: usize = 5;
pub const DEFAULT_OPTIONS: usize = 3;
pub const MIN_MINUTES: u64 = 1;
pub const MAX_MINUTES: u64 = 30;
pub const DEFAULT_MINUTES: u64 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vote {
    options: Vec<String>,
    initiator: u64,
    meal: Meal,
    minutes: u64,
    /// Each voter's current choice, by option index.
    ballots: HashMap<u64, usize>,
}

/// What a button press did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cast {
    Voted,
    Changed,
    /// Pressing your own choice again takes the vote back.
    Withdrawn,
}

/// How the winner was decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decided {
    Majority,
    Tie,
    NoVotes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    pub winner: usize,
    pub decided: Decided,
}

impl Vote {
    /// # Panics
    ///
    /// Panics unless there are `MIN_OPTIONS..=MAX_OPTIONS` options.
    pub fn new(options: Vec<String>, initiator: u64, meal: Meal, minutes: u64) -> Self {
        assert!(
            (MIN_OPTIONS..=MAX_OPTIONS).contains(&options.len()),
            "a vote needs {MIN_OPTIONS} to {MAX_OPTIONS} options"
        );
        Self {
            options,
            initiator,
            meal,
            minutes,
            ballots: HashMap::new(),
        }
    }

    pub fn options(&self) -> &[String] {
        &self.options
    }

    pub fn initiator(&self) -> u64 {
        self.initiator
    }

    /// Records a press on option `index`. Returns `None` for an unknown option.
    pub fn cast(&mut self, user_id: u64, index: usize) -> Option<Cast> {
        if index >= self.options.len() {
            return None;
        }
        match self.ballots.insert(user_id, index) {
            None => Some(Cast::Voted),
            Some(previous) if previous == index => {
                self.ballots.remove(&user_id);
                Some(Cast::Withdrawn)
            }
            Some(_) => Some(Cast::Changed),
        }
    }

    pub fn counts(&self) -> Vec<usize> {
        let mut counts = vec![0; self.options.len()];
        for index in self.ballots.values() {
            counts[*index] += 1;
        }
        counts
    }

    /// The most voted option. Ties, and a vote nobody joined, are settled by
    /// the 8-ball among the leading options.
    pub fn outcome(&self, rng: &mut impl RandomSource) -> Outcome {
        let counts = self.counts();
        let best = counts.iter().copied().max().unwrap_or(0);
        let leaders: Vec<usize> = (0..counts.len())
            .filter(|index| counts[*index] == best)
            .collect();
        let winner = *rng::pick(rng, &leaders).expect("a vote always has options");
        let decided = if best == 0 {
            Decided::NoVotes
        } else if leaders.len() > 1 {
            Decided::Tie
        } else {
            Decided::Majority
        };
        Outcome { winner, decided }
    }

    pub fn render_open(&self) -> String {
        format!(
            "揪團吃{}！{} 分鐘內按下面的按鈕投票，再按一次可以取消。\n{}",
            self.meal.label(),
            self.minutes,
            self.render_counts()
        )
    }

    pub fn render_closed(&self, outcome: Outcome) -> String {
        let winner = &self.options[outcome.winner];
        let verdict = match outcome.decided {
            Decided::Majority => {
                let votes = self.counts()[outcome.winner];
                format!("結果：{winner}（{votes} 票），就決定是它了！")
            }
            Decided::Tie => format!("平手，8-Ball 幫你們抽：{winner}。"),
            Decided::NoVotes => format!("沒有人投票，8-Ball 幫你們決定：{winner}。"),
        };
        format!(
            "揪團吃{}，投票結束！\n{}\n{verdict}",
            self.meal.label(),
            self.render_counts()
        )
    }

    fn render_counts(&self) -> String {
        self.options
            .iter()
            .zip(self.counts())
            .enumerate()
            .map(|(index, (option, count))| format!("{}. {option}　{count} 票", index + 1))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::tests::ScriptedRng;

    fn vote() -> Vote {
        Vote::new(
            vec!["火鍋".to_owned(), "拉麵".to_owned(), "壽司".to_owned()],
            1,
            Meal::Dinner,
            3,
        )
    }

    #[test]
    fn casting_changing_and_withdrawing() {
        let mut vote = vote();
        assert_eq!(vote.cast(10, 0), Some(Cast::Voted));
        assert_eq!(vote.cast(11, 0), Some(Cast::Voted));
        assert_eq!(vote.cast(10, 1), Some(Cast::Changed));
        assert_eq!(vote.counts(), vec![1, 1, 0]);
        assert_eq!(vote.cast(10, 1), Some(Cast::Withdrawn));
        assert_eq!(vote.counts(), vec![1, 0, 0]);
    }

    #[test]
    fn unknown_option_is_ignored() {
        let mut vote = vote();
        assert_eq!(vote.cast(10, 3), None);
        assert_eq!(vote.counts(), vec![0, 0, 0]);
    }

    #[test]
    fn majority_wins() {
        let mut vote = vote();
        vote.cast(10, 2);
        vote.cast(11, 2);
        vote.cast(12, 0);
        let outcome = vote.outcome(&mut ScriptedRng::zeros());
        assert_eq!(
            outcome,
            Outcome {
                winner: 2,
                decided: Decided::Majority
            }
        );
        assert!(vote.render_closed(outcome).contains("結果：壽司（2 票）"));
    }

    #[test]
    fn ties_pick_among_the_leaders() {
        let mut vote = vote();
        vote.cast(10, 1);
        vote.cast(11, 2);
        let first = vote.outcome(&mut ScriptedRng::zeros());
        let last = vote.outcome(&mut ScriptedRng::new(vec![u64::MAX]));
        assert_eq!(
            first,
            Outcome {
                winner: 1,
                decided: Decided::Tie
            }
        );
        assert_eq!(
            last,
            Outcome {
                winner: 2,
                decided: Decided::Tie
            }
        );
        assert!(vote.render_closed(first).contains("平手"));
    }

    #[test]
    fn no_votes_still_decides() {
        let vote = vote();
        let outcome = vote.outcome(&mut ScriptedRng::zeros());
        assert_eq!(outcome.decided, Decided::NoVotes);
        assert!(vote.render_closed(outcome).contains("沒有人投票"));
    }

    #[test]
    fn open_rendering_lists_every_option() {
        let mut vote = vote();
        vote.cast(10, 0);
        let text = vote.render_open();
        assert!(text.starts_with("揪團吃晚餐！3 分鐘內"));
        assert!(text.contains("1. 火鍋　1 票"));
        assert!(text.contains("3. 壽司　0 票"));
    }

    #[test]
    #[should_panic(expected = "a vote needs")]
    fn too_few_options_panics() {
        Vote::new(vec!["火鍋".to_owned()], 1, Meal::Dinner, 3);
    }

    #[test]
    #[should_panic(expected = "a vote needs")]
    fn too_many_options_panics() {
        let options = (0..=MAX_OPTIONS).map(|n| format!("菜{n}")).collect();
        Vote::new(options, 1, Meal::Dinner, 3);
    }
}
