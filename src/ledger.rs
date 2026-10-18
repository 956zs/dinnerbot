//! Remembers which bot reply answered which message, so replies can be
//! dismissed by their asker and cleaned up when the question is deleted.

use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplyRecord {
    pub reply_id: u64,
    /// The message that triggered the reply. `None` for slash commands.
    pub trigger_id: Option<u64>,
    pub owner_id: u64,
}

/// A bounded, oldest-first record of recent replies.
#[derive(Debug)]
pub struct ReplyLedger {
    capacity: usize,
    records: VecDeque<ReplyRecord>,
    /// Recently deleted messages that had no reply yet. Covers a question
    /// deleted while its answer was still being sent.
    deleted: VecDeque<u64>,
}

/// Deleted message IDs remembered while a reply may still be in flight.
const DELETED_CAPACITY: usize = 256;

impl ReplyLedger {
    /// A capacity of zero is treated as one.
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            capacity,
            records: VecDeque::with_capacity(capacity),
            deleted: VecDeque::with_capacity(DELETED_CAPACITY),
        }
    }

    /// Stores a reply. Returns `false` without storing it when its question
    /// was already deleted, in which case the reply should be removed too.
    pub fn insert(&mut self, record: ReplyRecord) -> bool {
        if let Some(trigger_id) = record.trigger_id {
            if let Some(index) = self.deleted.iter().position(|id| *id == trigger_id) {
                self.deleted.remove(index);
                return false;
            }
        }
        if self.records.len() == self.capacity {
            self.records.pop_front();
        }
        self.records.push_back(record);
        true
    }

    /// Handles a deleted message: forgets it if it was a reply, and returns
    /// the reply it triggered, if any, so that reply can be deleted too.
    pub fn message_deleted(&mut self, message_id: u64) -> Option<ReplyRecord> {
        if self.remove_reply(message_id).is_some() {
            return None;
        }
        let linked = self.take_by_trigger(message_id);
        if linked.is_none() {
            if self.deleted.len() == DELETED_CAPACITY {
                self.deleted.pop_front();
            }
            self.deleted.push_back(message_id);
        }
        linked
    }

    pub fn owner_of(&self, reply_id: u64) -> Option<u64> {
        self.records
            .iter()
            .find(|record| record.reply_id == reply_id)
            .map(|record| record.owner_id)
    }

    pub fn remove_reply(&mut self, reply_id: u64) -> Option<ReplyRecord> {
        let index = self
            .records
            .iter()
            .position(|record| record.reply_id == reply_id)?;
        self.records.remove(index)
    }

    pub fn take_by_trigger(&mut self, trigger_id: u64) -> Option<ReplyRecord> {
        let index = self
            .records
            .iter()
            .position(|record| record.trigger_id == Some(trigger_id))?;
        self.records.remove(index)
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(reply_id: u64, trigger_id: Option<u64>, owner_id: u64) -> ReplyRecord {
        ReplyRecord {
            reply_id,
            trigger_id,
            owner_id,
        }
    }

    #[test]
    fn finds_owner_by_reply() {
        let mut ledger = ReplyLedger::new(4);
        ledger.insert(record(10, Some(1), 100));
        assert_eq!(ledger.owner_of(10), Some(100));
        assert_eq!(ledger.owner_of(11), None);
    }

    #[test]
    fn evicts_the_oldest_when_full() {
        let mut ledger = ReplyLedger::new(2);
        ledger.insert(record(10, Some(1), 100));
        ledger.insert(record(11, Some(2), 101));
        ledger.insert(record(12, Some(3), 102));
        assert_eq!(ledger.len(), 2);
        assert_eq!(ledger.owner_of(10), None);
        assert_eq!(ledger.owner_of(12), Some(102));
    }

    #[test]
    fn zero_capacity_still_keeps_the_latest() {
        let mut ledger = ReplyLedger::new(0);
        ledger.insert(record(10, None, 100));
        ledger.insert(record(11, None, 101));
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger.owner_of(11), Some(101));
    }

    #[test]
    fn removes_by_reply_and_by_trigger() {
        let mut ledger = ReplyLedger::new(4);
        ledger.insert(record(10, Some(1), 100));
        ledger.insert(record(11, None, 101));

        assert_eq!(ledger.take_by_trigger(1), Some(record(10, Some(1), 100)));
        assert_eq!(ledger.take_by_trigger(1), None);
        assert_eq!(ledger.remove_reply(11), Some(record(11, None, 101)));
        assert_eq!(ledger.remove_reply(11), None);
        assert!(ledger.is_empty());
    }

    #[test]
    fn deleting_a_question_returns_its_reply() {
        let mut ledger = ReplyLedger::new(4);
        assert!(ledger.insert(record(10, Some(1), 100)));
        assert_eq!(ledger.message_deleted(1), Some(record(10, Some(1), 100)));
        assert!(ledger.is_empty());
    }

    #[test]
    fn deleting_a_reply_just_forgets_it() {
        let mut ledger = ReplyLedger::new(4);
        ledger.insert(record(10, Some(1), 100));
        assert_eq!(ledger.message_deleted(10), None);
        assert!(ledger.is_empty());
        // The reply's ID is not mistaken for a deleted question later.
        assert!(ledger.insert(record(11, Some(10), 100)));
    }

    #[test]
    fn reply_to_an_already_deleted_question_is_refused() {
        let mut ledger = ReplyLedger::new(4);
        assert_eq!(ledger.message_deleted(1), None);
        assert!(!ledger.insert(record(10, Some(1), 100)));
        assert!(ledger.is_empty());
        // Only refused once.
        assert!(ledger.insert(record(11, Some(1), 100)));
    }

    #[test]
    fn deleted_ids_are_bounded() {
        let mut ledger = ReplyLedger::new(4);
        for id in 0..=DELETED_CAPACITY as u64 {
            ledger.message_deleted(id);
        }
        // The oldest deleted ID was forgotten.
        assert!(ledger.insert(record(10, Some(0), 100)));
        assert!(!ledger.insert(record(11, Some(DELETED_CAPACITY as u64), 100)));
    }

    #[test]
    fn slash_replies_never_match_a_trigger() {
        let mut ledger = ReplyLedger::new(4);
        ledger.insert(record(10, None, 100));
        assert_eq!(ledger.take_by_trigger(10), None);
        assert_eq!(ledger.len(), 1);
    }
}
